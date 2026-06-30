//! Phase state machine driving pipeline stages (spec §3.1).

use crate::clock::Clock;
use crate::event_emit::{emit_query, emit_response, emit_retry};
use crate::phase::Phase;
use crate::pipeline::{PipelineStage, StageOutcome};
use crate::snapshot::RuntimeSnapshot;
use crate::transaction::Transaction;
use conduit_config::logging::log_text;
use conduit_events::EventHub;
use conduit_events::SelectorMatchCtx;
use conduit_metrics::{trace_activation_matches, MetricsHub, TracingHub};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

#[derive(Debug, PartialEq, Eq)]
pub enum RunOutcome {
    Response(Vec<u8>),
    Dropped,
}

/// Result of one orchestrator execution slice (complete or parked at suspend).
#[derive(Debug, PartialEq, Eq)]
pub enum OrchestratorRun {
    Finished(RunOutcome),
    Suspended { resume_phase: Phase },
}

impl OrchestratorRun {
    pub fn into_outcome(self) -> Option<RunOutcome> {
        match self {
            Self::Finished(o) => Some(o),
            Self::Suspended { .. } => None,
        }
    }
}

/// Query dnstap and pipeline-trace activation after request rules finish (including policy drop).
fn observe_after_request_rules(
    txn: &mut Transaction,
    snapshot: &Arc<RuntimeSnapshot>,
    events: Option<&EventHub>,
    tracing: Option<&TracingHub>,
) {
    if txn.qname.is_none() {
        return;
    }
    if txn.trace_log.is_none() {
        if let Some(th) = tracing {
            if snapshot.tracing_master_enabled() {
                let tag_has = |k: &str| txn.tags.has(k);
                let ctx = SelectorMatchCtx {
                    txn_id: txn.id,
                    global_query_index: 0,
                    qname: txn.qname.as_deref(),
                    qtype: txn.qtype,
                    rcode: txn.rcode(),
                    qclass: txn.qclass,
                    opcode: txn.opcode,
                    edns_option_codes: &txn.edns_option_codes,
                    tag_has: &tag_has,
                };
                if trace_activation_matches(&th.compiled.activation, &ctx) {
                    txn.trace_log = Some(conduit_metrics::TraceLog::default());
                }
            }
        }
    }
    if let Some(hub) = events {
        emit_query(hub, txn, snapshot);
    }
}

pub struct StageRegistry {
    stages: HashMap<Phase, Arc<dyn PipelineStage>>,
}

impl StageRegistry {
    pub fn new() -> Self {
        Self {
            stages: HashMap::new(),
        }
    }

    pub fn register(&mut self, phase: Phase, stage: Arc<dyn PipelineStage>) {
        self.stages.insert(phase, stage);
    }

    pub fn get(&self, phase: Phase) -> Option<Arc<dyn PipelineStage>> {
        self.stages.get(&phase).cloned()
    }
}

impl Default for StageRegistry {
    fn default() -> Self {
        Self::new()
    }
}

pub struct Orchestrator {
    pub registry: StageRegistry,
    pub metrics: Option<Arc<MetricsHub>>,
    pub tracing: Option<Arc<TracingHub>>,
}

impl Orchestrator {
    /// Run the full pipeline to completion (sync runtime). Suspends at WaitResponse are
    /// completed inline via the registered wait stage when present.
    pub fn run(
        &self,
        txn: &mut Transaction,
        snapshot: &Arc<RuntimeSnapshot>,
        clock: &dyn Clock,
        events: Option<&EventHub>,
    ) -> RunOutcome {
        let mut step = self.run_until_suspend(txn, snapshot, clock, events);
        loop {
            match step {
                OrchestratorRun::Finished(outcome) => return outcome,
                OrchestratorRun::Suspended { resume_phase } => {
                    step = self.resume_inline_wait(txn, snapshot, clock, events, resume_phase);
                }
            }
        }
    }

    /// Run until the pipeline completes or a stage returns [`StageOutcome::Suspend`].
    pub fn run_until_suspend(
        &self,
        txn: &mut Transaction,
        snapshot: &Arc<RuntimeSnapshot>,
        clock: &dyn Clock,
        events: Option<&EventHub>,
    ) -> OrchestratorRun {
        self.run_loop(txn, snapshot, clock, events, false)
    }

    /// Continue from a suspend point (typically after upstream I/O completes).
    pub fn resume_after_suspend(
        &self,
        txn: &mut Transaction,
        snapshot: &Arc<RuntimeSnapshot>,
        clock: &dyn Clock,
        events: Option<&EventHub>,
        resume_phase: Phase,
    ) -> OrchestratorRun {
        if resume_phase == Phase::WaitResponse {
            if let Some(hub) = self.metrics.as_ref() {
                if hub.metrics_enabled() {
                    if let Some(started) = txn.suspend_phase_started_at.take() {
                        hub.builtin.observe_phase(
                            phase_name(Phase::WaitResponse),
                            started.elapsed().as_secs_f64(),
                        );
                    }
                }
            } else {
                let _ = txn.suspend_phase_started_at.take();
            }
        }
        txn.current_phase = resume_phase;
        self.run_loop(txn, snapshot, clock, events, true)
    }

    fn resume_inline_wait(
        &self,
        txn: &mut Transaction,
        snapshot: &Arc<RuntimeSnapshot>,
        clock: &dyn Clock,
        events: Option<&EventHub>,
        resume_phase: Phase,
    ) -> OrchestratorRun {
        txn.current_phase = resume_phase;
        if let Some(stage) = self.registry.get(resume_phase) {
            let outcome = stage.handle(txn, snapshot);
            match outcome {
                StageOutcome::Drop => return OrchestratorRun::Finished(RunOutcome::Dropped),
                StageOutcome::Continue(next) => txn.current_phase = next,
                StageOutcome::Suspend(next) => {
                    return OrchestratorRun::Suspended { resume_phase: next };
                }
            }
        } else {
            txn.current_phase = next_phase(resume_phase);
        }
        self.run_until_suspend(txn, snapshot, clock, events)
    }

    fn run_loop(
        &self,
        txn: &mut Transaction,
        snapshot: &Arc<RuntimeSnapshot>,
        _clock: &dyn Clock,
        events: Option<&EventHub>,
        resumed: bool,
    ) -> OrchestratorRun {
        let metrics = self.metrics.as_deref();
        let tracing = self.tracing.as_deref();
        let max_attempts = snapshot
            .config
            .orchestrator
            .as_ref()
            .map(|o| o.max_attempts)
            .unwrap_or(3);
        let max_duration = snapshot
            .config
            .orchestrator
            .as_ref()
            .map(|o| o.max_txn_duration_ms)
            .unwrap_or(5000);

        if !resumed {
            txn.snapshot_generation = snapshot.generation;
            txn.current_phase = Phase::Parse;
        }

        loop {
            if txn.started_at.elapsed() > Duration::from_millis(max_duration as u64) {
                txn.set_rcode_name("SERVFAIL");
                txn.current_phase = Phase::Send;
            }

            if txn.current_phase == Phase::Route && txn.attempt_count >= max_attempts {
                txn.set_rcode_name("SERVFAIL");
                txn.current_phase = Phase::Send;
            }

            let Some(stage) = self.registry.get(txn.current_phase) else {
                if txn.current_phase == Phase::Send {
                    if let Some(hub) = metrics {
                        if hub.metrics_enabled() {
                            let protocol = match txn.protocol {
                                crate::transaction::ClientProtocol::Udp => "udp",
                                crate::transaction::ClientProtocol::Tcp => "tcp",
                            };
                            let listener = txn.listener_label.as_deref().unwrap_or("unknown");
                            hub.builtin.record_response(
                                listener,
                                protocol,
                                txn.rcode(),
                                &txn.client_addr,
                            );
                        }
                    }
                    if let Some(hub) = events {
                        emit_response(hub, txn, snapshot);
                        emit_retry(hub, txn, snapshot);
                    }
                    break;
                }
                txn.current_phase = next_phase(txn.current_phase);
                continue;
            };

            let phase = txn.current_phase;
            let phase_started = std::time::Instant::now();
            let skip_post_phase_observe =
                resumed && phase == Phase::WaitResponse && txn.suspend_phase_started_at.is_none();
            let outcome = stage.handle(txn, snapshot);
            if let Some(hub) = metrics {
                if hub.metrics_enabled() && !skip_post_phase_observe {
                    hub.builtin
                        .observe_phase(phase_name(phase), phase_started.elapsed().as_secs_f64());
                }
            }
            txn.trace_record_phase(
                phase_name(phase),
                None,
                txn.selected_pool.clone(),
                txn.selected_backend_display(),
            );
            if phase == Phase::RequestRules {
                observe_after_request_rules(txn, snapshot, events, tracing);
            }
            match outcome {
                StageOutcome::Drop => {
                    if let Some(hub) = metrics {
                        if hub.metrics_enabled() && phase == Phase::Parse {
                            if let Some(reason) = txn.parse_reject_reason {
                                hub.builtin.record_parse_rejected(reason.as_str());
                            }
                        }
                    }
                    return OrchestratorRun::Finished(RunOutcome::Dropped);
                }
                StageOutcome::Suspend(resume_phase) => {
                    txn.suspend_phase_started_at = Some(std::time::Instant::now());
                    return OrchestratorRun::Suspended { resume_phase };
                }
                StageOutcome::Continue(next) => {
                    if phase == Phase::Parse && next == Phase::RequestRules {
                        if let Some(hub) = metrics {
                            if hub.metrics_enabled() {
                                let protocol = match txn.protocol {
                                    crate::transaction::ClientProtocol::Udp => "udp",
                                    crate::transaction::ClientProtocol::Tcp => "tcp",
                                };
                                let listener = txn.listener_label.as_deref().unwrap_or("unknown");
                                hub.builtin.record_query(
                                    listener,
                                    protocol,
                                    txn.qtype,
                                    txn.qclass,
                                    &txn.client_addr,
                                );
                            }
                        }
                    }
                    if phase == Phase::Route && next == Phase::Forward {
                        if let Some(hub) = metrics {
                            if hub.metrics_enabled() {
                                if let Some(ref pool) = txn.selected_pool {
                                    hub.builtin.record_query_by_pool(pool);
                                }
                            }
                        }
                    }
                    if phase == Phase::ResponseRules && next == Phase::Route {
                        if let Some(hub) = metrics {
                            let retry_target =
                                txn.retry_pool.as_ref().or(txn.selected_pool.as_ref());
                            if let Some(pool) = retry_target {
                                hub.builtin.record_retry(pool);
                            }
                        }
                    }
                    txn.current_phase = next;
                    if phase == Phase::Send {
                        if let Some(hub) = metrics {
                            if hub.metrics_enabled() {
                                let protocol = match txn.protocol {
                                    crate::transaction::ClientProtocol::Udp => "udp",
                                    crate::transaction::ClientProtocol::Tcp => "tcp",
                                };
                                let listener = txn.listener_label.as_deref().unwrap_or("unknown");
                                hub.builtin.record_response(
                                    listener,
                                    protocol,
                                    txn.rcode(),
                                    &txn.client_addr,
                                );
                            }
                        }
                        if let Some(hub) = events {
                            emit_response(hub, txn, snapshot);
                            emit_retry(hub, txn, snapshot);
                        }
                        break;
                    }
                }
            }
        }

        let outcome = if let Some(wire) = txn.response_wire.clone() {
            RunOutcome::Response(wire)
        } else {
            RunOutcome::Dropped
        };

        self.finalize_trace(txn, tracing);
        self.log_run_outcome(txn, &outcome);
        OrchestratorRun::Finished(outcome)
    }

    fn finalize_trace(&self, txn: &mut Transaction, tracing: Option<&TracingHub>) {
        if let Some(th) = tracing {
            if let Some(log) = txn.trace_log.take() {
                if !log.events.is_empty() {
                    th.store.insert(txn.id, log.events.clone());
                    if th.compiled.log_json {
                        if let Ok(json) = serde_json::to_string(&log.events) {
                            tracing::info!(
                                target: "conduit::trace",
                                txn_id = txn.id,
                                events = %json,
                                "pipeline trace"
                            );
                        }
                    }
                }
            }
        }
    }

    fn log_run_outcome(&self, txn: &Transaction, outcome: &RunOutcome) {
        match outcome {
            RunOutcome::Response(_) => tracing::debug!(
                txn_id = txn.id,
                dns_id = txn.dns_id,
                qname = %log_text(txn.qname.as_deref().unwrap_or("-")),
                rcode = %log_text(txn.rcode_label().as_deref().unwrap_or("-")),
                pool = %log_text(txn.selected_pool.as_deref().unwrap_or("-")),
                backend = %log_text(
                    &txn
                        .selected_backend_display()
                        .unwrap_or_else(|| "-".into())
                ),
                attempts = txn.attempt_count,
                "query complete"
            ),
            RunOutcome::Dropped => tracing::debug!(
                txn_id = txn.id,
                dns_id = txn.dns_id,
                qname = %log_text(txn.qname.as_deref().unwrap_or("-")),
                "query dropped"
            ),
        }
    }
}

fn phase_name(phase: Phase) -> &'static str {
    match phase {
        Phase::Receive => "receive",
        Phase::Parse => "parse",
        Phase::RequestRules => "request_rules",
        Phase::Route => "route",
        Phase::Forward => "forward",
        Phase::WaitResponse => "wait_response",
        Phase::ResponseRules => "response_rules",
        Phase::Send => "send",
    }
}

fn next_phase(phase: Phase) -> Phase {
    match phase {
        Phase::Receive => Phase::Parse,
        Phase::Parse => Phase::RequestRules,
        Phase::RequestRules => Phase::Route,
        Phase::Route => Phase::Forward,
        Phase::Forward => Phase::WaitResponse,
        Phase::WaitResponse => Phase::ResponseRules,
        Phase::ResponseRules => Phase::Send,
        Phase::Send => Phase::Send,
    }
}

impl Orchestrator {
    pub fn with_default_stages() -> Self {
        use crate::stages::{
            ParseStage, RequestRulesStage, ResponseRulesStage, RouteStage, SendStage,
        };
        let mut registry = StageRegistry::new();
        registry.register(Phase::Parse, Arc::new(ParseStage));
        registry.register(Phase::RequestRules, Arc::new(RequestRulesStage::default()));
        registry.register(Phase::Route, Arc::new(RouteStage::new()));
        registry.register(
            Phase::ResponseRules,
            Arc::new(ResponseRulesStage::default()),
        );
        registry.register(Phase::Send, Arc::new(SendStage));
        Self {
            registry,
            metrics: None,
            tracing: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clock::SystemClock;
    use crate::pipeline::{PipelineStage, StageOutcome};
    use crate::transaction::ClientProtocol;
    use conduit_config::load_yaml;
    use hickory_proto::op::ResponseCode;
    use hickory_proto::op::{Message, Query};
    use hickory_proto::rr::{Name, RecordType};
    use hickory_proto::serialize::binary::{BinEncodable, BinEncoder};
    use std::path::PathBuf;

    fn fixtures_config_base() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/config")
    }

    fn snapshot_from_fixture(yaml: &str) -> Arc<RuntimeSnapshot> {
        let cfg = load_yaml(yaml).unwrap();
        assert!(conduit_config::validate(&cfg).ok);
        Arc::new(RuntimeSnapshot::from_config_with_base(
            cfg,
            Some(&fixtures_config_base()),
        ))
    }

    fn query_for(name: &str) -> Vec<u8> {
        let name = Name::from_utf8(name).unwrap();
        let query = Query::query(name, RecordType::A);
        let mut msg = Message::new();
        msg.add_query(query);
        let mut buf = Vec::new();
        let mut encoder = BinEncoder::new(&mut buf);
        msg.emit(&mut encoder).unwrap();
        buf
    }

    struct MockForwardStage;

    impl PipelineStage for MockForwardStage {
        fn name(&self) -> &'static str {
            "mock_forward"
        }

        fn handle(&self, txn: &mut Transaction, _snapshot: &Arc<RuntimeSnapshot>) -> StageOutcome {
            let mut msg = Message::new();
            msg.set_id(txn.dns_id);
            msg.set_response_code(ResponseCode::ServFail);
            let mut buf = Vec::new();
            let mut encoder = BinEncoder::new(&mut buf);
            msg.emit(&mut encoder).unwrap();
            txn.response_wire = Some(buf);
            txn.set_rcode(2);
            StageOutcome::Continue(Phase::WaitResponse)
        }
    }

    struct PassthroughWait;

    impl PipelineStage for PassthroughWait {
        fn name(&self) -> &'static str {
            "wait"
        }

        fn handle(&self, _txn: &mut Transaction, _snapshot: &Arc<RuntimeSnapshot>) -> StageOutcome {
            StageOutcome::Continue(Phase::ResponseRules)
        }
    }

    struct MockForwardNoResponse;

    impl PipelineStage for MockForwardNoResponse {
        fn name(&self) -> &'static str {
            "mock_forward_no_response"
        }

        fn handle(&self, txn: &mut Transaction, _snapshot: &Arc<RuntimeSnapshot>) -> StageOutcome {
            txn.set_rcode_name("SERVFAIL");
            StageOutcome::Continue(Phase::ResponseRules)
        }
    }

    fn orchestrator_with_forward_no_response() -> Orchestrator {
        let mut o = Orchestrator::with_default_stages();
        o.registry
            .register(Phase::Forward, Arc::new(MockForwardNoResponse));
        o.registry
            .register(Phase::WaitResponse, Arc::new(PassthroughWait));
        o
    }

    struct SuspendForwardStage;

    impl PipelineStage for SuspendForwardStage {
        fn name(&self) -> &'static str {
            "suspend_forward"
        }

        fn handle(&self, _txn: &mut Transaction, _snapshot: &Arc<RuntimeSnapshot>) -> StageOutcome {
            StageOutcome::Suspend(Phase::WaitResponse)
        }
    }

    fn orchestrator_with_suspend_forward() -> Orchestrator {
        let mut o = Orchestrator::with_default_stages();
        o.registry
            .register(Phase::Forward, Arc::new(SuspendForwardStage));
        o.registry
            .register(Phase::WaitResponse, Arc::new(PassthroughWait));
        o
    }

    #[test]
    fn suspend_sets_park_timestamp() {
        let yaml = include_str!("../../../tests/fixtures/config/minimal.yaml");
        let cfg = load_yaml(yaml).unwrap();
        let snap = Arc::new(RuntimeSnapshot::from_config(cfg));
        let mut txn = Transaction::new(1, "127.0.0.1:15353".parse().unwrap(), ClientProtocol::Udp)
            .with_query_wire(example_query());
        let orch = orchestrator_with_suspend_forward();
        assert!(matches!(
            orch.run_until_suspend(&mut txn, &snap, &SystemClock, None),
            OrchestratorRun::Suspended { .. }
        ));
        assert!(txn.suspend_phase_started_at.is_some());
    }

    #[test]
    fn resume_clears_park_timestamp() {
        let yaml = include_str!("../../../tests/fixtures/config/minimal.yaml");
        let cfg = load_yaml(yaml).unwrap();
        let snap = Arc::new(RuntimeSnapshot::from_config(cfg));
        let mut txn = Transaction::new(1, "127.0.0.1:15353".parse().unwrap(), ClientProtocol::Udp)
            .with_query_wire(example_query());
        let orch = orchestrator_with_suspend_forward();
        assert!(matches!(
            orch.run_until_suspend(&mut txn, &snap, &SystemClock, None),
            OrchestratorRun::Suspended { .. }
        ));
        let mut msg = Message::new();
        msg.set_id(txn.dns_id);
        msg.set_response_code(ResponseCode::NoError);
        let mut buf = Vec::new();
        let mut encoder = BinEncoder::new(&mut buf);
        msg.emit(&mut encoder).unwrap();
        txn.response_wire = Some(buf);
        let _ = orch.resume_after_suspend(&mut txn, &snap, &SystemClock, None, Phase::WaitResponse);
        assert!(txn.suspend_phase_started_at.is_none());
    }

    #[test]
    fn suspend_at_forward_parks_at_wait_response() {
        let yaml = include_str!("../../../tests/fixtures/config/minimal.yaml");
        let cfg = load_yaml(yaml).unwrap();
        let snap = Arc::new(RuntimeSnapshot::from_config(cfg));
        let mut txn = Transaction::new(1, "127.0.0.1:15353".parse().unwrap(), ClientProtocol::Udp)
            .with_query_wire(example_query());
        let orch = orchestrator_with_suspend_forward();
        let step = orch.run_until_suspend(&mut txn, &snap, &SystemClock, None);
        assert_eq!(
            step,
            OrchestratorRun::Suspended {
                resume_phase: Phase::WaitResponse
            }
        );
        assert_eq!(txn.current_phase, Phase::Forward);
    }

    #[test]
    fn resume_after_upstream_reply_completes() {
        let yaml = include_str!("../../../tests/fixtures/config/minimal.yaml");
        let cfg = load_yaml(yaml).unwrap();
        let snap = Arc::new(RuntimeSnapshot::from_config(cfg));
        let mut txn = Transaction::new(1, "127.0.0.1:15353".parse().unwrap(), ClientProtocol::Udp)
            .with_query_wire(example_query());
        let orch = orchestrator_with_suspend_forward();
        assert!(matches!(
            orch.run_until_suspend(&mut txn, &snap, &SystemClock, None),
            OrchestratorRun::Suspended { .. }
        ));

        let mut msg = Message::new();
        msg.set_id(txn.dns_id);
        msg.set_response_code(ResponseCode::NoError);
        let mut buf = Vec::new();
        let mut encoder = BinEncoder::new(&mut buf);
        msg.emit(&mut encoder).unwrap();
        txn.response_wire = Some(buf);

        let step =
            orch.resume_after_suspend(&mut txn, &snap, &SystemClock, None, Phase::WaitResponse);
        assert!(matches!(
            step,
            OrchestratorRun::Finished(RunOutcome::Response(_))
        ));
    }

    #[test]
    fn max_txn_duration_includes_parked_wait() {
        use std::thread;
        use std::time::Duration;

        let yaml = include_str!("../../../tests/fixtures/config/minimal.yaml");
        let mut cfg = load_yaml(yaml).unwrap();
        cfg.orchestrator.as_mut().unwrap().max_txn_duration_ms = 50;
        let snap = Arc::new(RuntimeSnapshot::from_config(cfg));
        let mut txn = Transaction::new(1, "127.0.0.1:15353".parse().unwrap(), ClientProtocol::Udp)
            .with_query_wire(example_query());
        let orch = orchestrator_with_suspend_forward();
        assert!(matches!(
            orch.run_until_suspend(&mut txn, &snap, &SystemClock, None),
            OrchestratorRun::Suspended { .. }
        ));
        thread::sleep(Duration::from_millis(80));
        let step =
            orch.resume_after_suspend(&mut txn, &snap, &SystemClock, None, Phase::WaitResponse);
        match step {
            OrchestratorRun::Finished(RunOutcome::Response(_)) => {
                assert_eq!(txn.rcode_label().as_deref(), Some("SERVFAIL"));
            }
            other => panic!("expected SERVFAIL after parked timeout, got {other:?}"),
        }
    }

    #[test]
    fn retry_loop_survives_suspend_and_resume() {
        use std::sync::atomic::{AtomicU32, Ordering};

        struct SuspendOnceThenRespond {
            calls: AtomicU32,
        }

        impl PipelineStage for SuspendOnceThenRespond {
            fn name(&self) -> &'static str {
                "suspend_once_forward"
            }

            fn handle(
                &self,
                txn: &mut Transaction,
                _snapshot: &Arc<RuntimeSnapshot>,
            ) -> StageOutcome {
                if self.calls.fetch_add(1, Ordering::Relaxed) == 0 {
                    return StageOutcome::Suspend(Phase::WaitResponse);
                }
                let mut msg = Message::new();
                msg.set_id(txn.dns_id);
                msg.set_response_code(ResponseCode::ServFail);
                let mut buf = Vec::new();
                let mut encoder = BinEncoder::new(&mut buf);
                msg.emit(&mut encoder).unwrap();
                txn.response_wire = Some(buf);
                txn.set_rcode(2);
                StageOutcome::Continue(Phase::WaitResponse)
            }
        }

        let yaml = include_str!("../../../tests/fixtures/config/with-rules.yaml");
        let cfg = load_yaml(yaml).unwrap();
        let snap = Arc::new(RuntimeSnapshot::from_config(cfg));
        let mut txn = Transaction::new(2, "127.0.0.1:15353".parse().unwrap(), ClientProtocol::Udp)
            .with_query_wire(example_query());
        let mut orch = Orchestrator::with_default_stages();
        orch.registry.register(
            Phase::Forward,
            Arc::new(SuspendOnceThenRespond {
                calls: AtomicU32::new(0),
            }),
        );
        orch.registry
            .register(Phase::WaitResponse, Arc::new(PassthroughWait));

        assert!(matches!(
            orch.run_until_suspend(&mut txn, &snap, &SystemClock, None),
            OrchestratorRun::Suspended { .. }
        ));

        let mut msg = Message::new();
        msg.set_id(txn.dns_id);
        msg.set_response_code(ResponseCode::ServFail);
        let mut buf = Vec::new();
        let mut encoder = BinEncoder::new(&mut buf);
        msg.emit(&mut encoder).unwrap();
        txn.response_wire = Some(buf);
        txn.set_rcode(2);

        let step =
            orch.resume_after_suspend(&mut txn, &snap, &SystemClock, None, Phase::WaitResponse);
        assert!(matches!(step, OrchestratorRun::Finished(_)));
        assert!(txn.attempts.len() >= 2, "attempts={:?}", txn.attempts);
    }

    #[test]
    fn upstream_timeout_path_returns_servfail_wire() {
        let yaml = include_str!("../../../tests/fixtures/config/with-rhai-vip-pool.yaml");
        let cfg = load_yaml(yaml).unwrap();
        let base = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures/config");
        let snap = Arc::new(RuntimeSnapshot::try_from_config_with_base(cfg, Some(&base)).unwrap());
        let mut txn = Transaction::new(99, "127.0.0.1:15353".parse().unwrap(), ClientProtocol::Udp)
            .with_query_wire(query_for("foo.vip.example."));
        let orch = orchestrator_with_forward_no_response();
        let outcome = orch.run(&mut txn, &snap, &SystemClock, None);
        match outcome {
            RunOutcome::Response(wire) => assert!(!wire.is_empty()),
            RunOutcome::Dropped => panic!("expected synthesized SERVFAIL response"),
        }
        assert_eq!(txn.rcode_label().as_deref(), Some("SERVFAIL"));
    }

    fn example_query() -> Vec<u8> {
        let name = Name::from_utf8("test.example.com.").unwrap();
        let query = Query::query(name, RecordType::A);
        let mut msg = Message::new();
        msg.add_query(query);
        let mut buf = Vec::new();
        let mut encoder = BinEncoder::new(&mut buf);
        msg.emit(&mut encoder).unwrap();
        buf
    }

    fn orchestrator_with_mock_forward() -> Orchestrator {
        let mut o = Orchestrator::with_default_stages();
        o.registry
            .register(Phase::Forward, Arc::new(MockForwardStage));
        o.registry
            .register(Phase::WaitResponse, Arc::new(PassthroughWait));
        o
    }

    #[test]
    fn happy_path_produces_response() {
        let yaml = include_str!("../../../tests/fixtures/config/minimal.yaml");
        let cfg = load_yaml(yaml).unwrap();
        let snap = Arc::new(RuntimeSnapshot::from_config(cfg));
        let mut txn = Transaction::new(1, "127.0.0.1:15353".parse().unwrap(), ClientProtocol::Udp)
            .with_query_wire(example_query());
        let orch = orchestrator_with_mock_forward();
        let outcome = orch.run(&mut txn, &snap, &SystemClock, None);
        assert!(matches!(outcome, RunOutcome::Response(_)));
    }

    #[test]
    fn servfail_retry_uses_secondary_pool() {
        let yaml = include_str!("../../../tests/fixtures/config/with-rules.yaml");
        let cfg = load_yaml(yaml).unwrap();
        let snap = Arc::new(RuntimeSnapshot::from_config(cfg));
        let mut txn = Transaction::new(2, "127.0.0.1:15353".parse().unwrap(), ClientProtocol::Udp)
            .with_query_wire(example_query());
        let orch = orchestrator_with_mock_forward();
        let _ = orch.run(&mut txn, &snap, &SystemClock, None);
        assert!(txn.attempts.len() >= 2);
        assert_eq!(txn.attempts[0].pool, "primary");
        assert_eq!(txn.attempts.last().unwrap().pool, "secondary");
    }

    #[test]
    fn servfail_same_pool_retry_uses_different_backends() {
        let yaml = include_str!("../../../tests/fixtures/config/with-same-pool-retry.yaml");
        let cfg = load_yaml(yaml).unwrap();
        let snap = Arc::new(RuntimeSnapshot::from_config(cfg));
        let mut txn = Transaction::new(2, "127.0.0.1:15353".parse().unwrap(), ClientProtocol::Udp)
            .with_query_wire(example_query());
        let orch = orchestrator_with_mock_forward();
        let _ = orch.run(&mut txn, &snap, &SystemClock, None);
        assert_eq!(txn.attempts.len(), 3, "attempts={:?}", txn.attempts);
        assert_eq!(txn.attempts[0].pool, "primary");
        assert_eq!(txn.attempts[1].pool, "primary");
        assert_eq!(txn.attempts[2].pool, "primary");
        let backends: Vec<_> = txn.attempts.iter().map(|a| a.backend).collect();
        assert_eq!(backends.len(), 3);
        assert_ne!(backends[0], backends[1]);
        assert_ne!(backends[0], backends[2]);
        assert_ne!(backends[1], backends[2]);
    }

    #[test]
    fn query_observation_after_request_rules_respects_tag_required() {
        use conduit_events::EventHub;

        let yaml = include_str!("../../../tests/fixtures/config/with-dnstap-filters.yaml");
        let cfg = load_yaml(yaml).unwrap();
        let snap = Arc::new(RuntimeSnapshot::from_config(cfg));
        let hub = EventHub::from_compiled(&snap.events);
        let orch = orchestrator_with_mock_forward();

        let name = Name::from_utf8("pay.payments.corp.example.").unwrap();
        let query = Query::query(name, RecordType::A);
        let mut msg = Message::new();
        msg.add_query(query);
        let mut buf = Vec::new();
        let mut encoder = BinEncoder::new(&mut buf);
        msg.emit(&mut encoder).unwrap();

        let mut txn = Transaction::new(10, "127.0.0.1:15353".parse().unwrap(), ClientProtocol::Udp)
            .with_query_wire(buf);
        let _ = orch.run(&mut txn, &snap, &SystemClock, Some(&hub));
        let metrics = hub.sink_metrics_snapshot();
        assert!(
            metrics[0].enqueued_query >= 1,
            "tag set in request rules should allow query export"
        );

        let mut txn2 =
            Transaction::new(11, "127.0.0.1:15353".parse().unwrap(), ClientProtocol::Udp)
                .with_query_wire(example_query());
        let _ = orch.run(&mut txn2, &snap, &SystemClock, Some(&hub));
        let metrics2 = hub.sink_metrics_snapshot();
        assert_eq!(
            metrics2[0].enqueued_query, metrics[0].enqueued_query,
            "query without audit tag should not enqueue"
        );
    }

    #[test]
    fn query_observation_after_request_rules_on_policy_drop() {
        use conduit_events::EventHub;

        let yaml = include_str!("../../../tests/fixtures/config/with-rhai-blocklist-dnstap.yaml");
        let snap = snapshot_from_fixture(yaml);
        let hub = EventHub::from_compiled(&snap.events);
        let orch = orchestrator_with_mock_forward();

        let mut txn = Transaction::new(21, "127.0.0.1:15353".parse().unwrap(), ClientProtocol::Udp)
            .with_query_wire(query_for("bad.example."));
        let outcome = orch.run(&mut txn, &snap, &SystemClock, Some(&hub));
        assert!(matches!(outcome, RunOutcome::Dropped));
        let metrics = hub.sink_metrics_snapshot();
        assert!(
            metrics[0].enqueued_query >= 1,
            "request-hook policy drop should still enqueue query observation"
        );
    }

    #[test]
    fn rhai_blocklist_drops_blocked_name() {
        let yaml = include_str!("../../../tests/fixtures/config/with-rhai-blocklist.yaml");
        let snap = snapshot_from_fixture(yaml);
        let mut txn = Transaction::new(20, "127.0.0.1:15353".parse().unwrap(), ClientProtocol::Udp)
            .with_query_wire(query_for("bad.example."));
        let orch = orchestrator_with_mock_forward();
        let outcome = orch.run(&mut txn, &snap, &SystemClock, None);
        assert!(matches!(outcome, RunOutcome::Dropped));
    }

    #[test]
    fn rhai_servfail_retry_uses_secondary_pool() {
        let yaml = include_str!("../../../tests/fixtures/config/with-rhai-servfail-retry.yaml");
        let snap = snapshot_from_fixture(yaml);
        let mut txn = Transaction::new(21, "127.0.0.1:15353".parse().unwrap(), ClientProtocol::Udp)
            .with_query_wire(example_query());
        let orch = orchestrator_with_mock_forward();
        let _ = orch.run(&mut txn, &snap, &SystemClock, None);
        assert!(
            txn.attempts.len() >= 2,
            "attempts={:?} count={}",
            txn.attempts,
            txn.attempt_count
        );
        assert_eq!(txn.attempts[0].pool, "primary");
        assert_eq!(txn.attempts.last().unwrap().pool, "secondary");
    }

    #[test]
    fn rhai_dnstap_tag_gates_query_export() {
        use conduit_events::EventHub;

        let yaml = include_str!("../../../tests/fixtures/config/with-rhai-dnstap-tag.yaml");
        let snap = snapshot_from_fixture(yaml);
        let hub = EventHub::from_compiled(&snap.events);
        let orch = orchestrator_with_mock_forward();

        let mut txn = Transaction::new(22, "127.0.0.1:15353".parse().unwrap(), ClientProtocol::Udp)
            .with_query_wire(query_for("foo.audit.example."));
        let _ = orch.run(&mut txn, &snap, &SystemClock, Some(&hub));
        let metrics = hub.sink_metrics_snapshot();
        assert!(metrics[0].enqueued_query >= 1);

        let mut txn2 =
            Transaction::new(23, "127.0.0.1:15353".parse().unwrap(), ClientProtocol::Udp)
                .with_query_wire(example_query());
        let _ = orch.run(&mut txn2, &snap, &SystemClock, Some(&hub));
        let metrics2 = hub.sink_metrics_snapshot();
        assert_eq!(metrics2[0].enqueued_query, metrics[0].enqueued_query);
    }

    #[test]
    fn rhai_sample_percent_stable_per_txn() {
        use conduit_events::hash_sample;

        let yaml = include_str!("../../../tests/fixtures/config/with-rhai-sample.yaml");
        let snap = snapshot_from_fixture(yaml);
        let txn_id = 4242_u64;
        let percent = 5.0;
        let expected = hash_sample(txn_id, percent / 100.0);

        let scripting = &snap.scripting;
        let script_id = scripting.rules_scripts[0].script_id;
        let mut host = crate::transaction::Transaction::new(
            txn_id,
            "127.0.0.1:15353".parse().unwrap(),
            ClientProtocol::Udp,
        );
        host.qname = Some("test.example.".into());
        host.qtype = Some(1);
        let ids = vec![script_id];
        let (_, _) = conduit_script::run_scripts(
            scripting,
            &ids,
            &mut host,
            conduit_script::ScriptPhase::Request,
            None,
            None,
            None,
        );
        assert_eq!(host.tags.has("sampled"), expected);
    }
}
