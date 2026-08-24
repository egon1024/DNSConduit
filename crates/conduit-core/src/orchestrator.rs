//! Phase state machine driving pipeline stages (spec §3.1).

use crate::clock::Clock;
use crate::event_emit::{emit_query, emit_response, emit_retry};
use crate::phase::Phase;
use crate::pipeline::{PipelineStage, StageOutcome};
use crate::selector_ctx::selector_match_ctx;
use crate::snapshot::RuntimeSnapshot;
use crate::transaction::Transaction;
use conduit_config::logging::log_text;
use conduit_events::EventHub;
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
                let store = &snapshot.scripting.data_sources;
                let client_ip = txn.client_addr.ip();
                let client_cidr_match = |name: &str| store.lookup_ip(name, client_ip).is_some();
                let ctx = selector_match_ctx(txn, &tag_has, Some(&client_cidr_match));
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
        if resume_phase == Phase::Lookup {
            if let Some(hub) = self.metrics.as_ref() {
                if hub.metrics_enabled() {
                    if let Some(started) = txn.suspend_phase_started_at.take() {
                        txn.builtin_registry(hub).observe_phase(
                            phase_name(Phase::Lookup),
                            started.elapsed().as_secs_f64(),
                        );
                    }
                }
            } else {
                let _ = txn.suspend_phase_started_at.take();
            }
        } else if resume_phase == Phase::WaitResponse {
            // Legacy internal resume (tests only).
            if let Some(hub) = self.metrics.as_ref() {
                if hub.metrics_enabled() {
                    if let Some(started) = txn.suspend_phase_started_at.take() {
                        txn.builtin_registry(hub).observe_phase(
                            phase_name(Phase::Lookup),
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
        let max_duration = snapshot
            .config
            .orchestrator
            .as_ref()
            .map(|o| o.max_txn_duration_ms)
            .unwrap_or(5000);

        if !resumed {
            txn.snapshot_generation = snapshot.generation;
            txn.current_phase = Phase::Parse;
            // Pin metrics registries for this txn so mid-flight plan swaps do not
            // change label schemas or drop handles under an in-flight query.
            txn.metrics_pin = self.metrics.as_ref().map(|h| h.acquire_pin());
        }

        loop {
            if txn.started_at.elapsed() > Duration::from_millis(max_duration as u64)
                && !matches!(txn.current_phase, Phase::NoAnswer | Phase::Send)
            {
                txn.set_rcode_name("SERVFAIL");
                if txn.response_wire.is_none() {
                    txn.set_convergence_reason(
                        crate::transaction::ConvergenceReason::DurationExhausted,
                    );
                    txn.current_phase = Phase::NoAnswer;
                } else {
                    txn.current_phase = Phase::Send;
                }
            }

            let Some(stage) = self.registry.get(txn.current_phase) else {
                if txn.current_phase == Phase::Send {
                    if let Some(hub) = metrics {
                        if hub.metrics_enabled() {
                            record_send_completion_metrics(hub, txn);
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
            let skip_post_phase_observe = resumed
                && (phase == Phase::Lookup || phase == Phase::WaitResponse)
                && txn.suspend_phase_started_at.is_none();
            let outcome = stage.handle(txn, snapshot);
            if let Some(hub) = metrics {
                if hub.metrics_enabled() && !skip_post_phase_observe {
                    txn.builtin_registry(hub)
                        .observe_phase(phase_name(phase), phase_started.elapsed().as_secs_f64());
                }
            }
            txn.trace_record_phase(
                phase_name(phase),
                None,
                txn.selected_pool.clone(),
                txn.selected_backend_display(),
                None,
            );
            if phase == Phase::RequestRules {
                observe_after_request_rules(txn, snapshot, events, tracing);
            }
            match outcome {
                StageOutcome::Drop => {
                    if let Some(hub) = metrics {
                        if hub.metrics_enabled() {
                            match phase {
                                Phase::Parse => {
                                    if let Some(reason) = txn.parse_reject_reason {
                                        txn.builtin_registry(hub)
                                            .record_parse_rejected(reason.as_str());
                                    }
                                }
                                Phase::RequestRules | Phase::ResponseRules | Phase::NoAnswer => {
                                    let protocol = match txn.protocol {
                                        crate::transaction::ClientProtocol::Udp => "udp",
                                        crate::transaction::ClientProtocol::Tcp => "tcp",
                                    };
                                    let listener =
                                        txn.listener_label.as_deref().unwrap_or("unknown");
                                    let reason = match phase {
                                        Phase::RequestRules => "request_rules",
                                        Phase::ResponseRules => "response_rules",
                                        Phase::NoAnswer => "no_answer_rules",
                                        _ => unreachable!(),
                                    };
                                    txn.builtin_registry(hub).record_query_dropped(
                                        listener,
                                        protocol,
                                        reason,
                                        &txn.client_addr,
                                    );
                                }
                                _ => {}
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
                                txn.builtin_registry(hub).record_query(
                                    listener,
                                    protocol,
                                    txn.qtype,
                                    txn.qclass,
                                    &txn.client_addr,
                                );
                            }
                        }
                    }
                    if phase == Phase::ResponseRules && next == Phase::Lookup {
                        if let Some(hub) = metrics {
                            let retry_target =
                                txn.retry_pool.as_ref().or(txn.selected_pool.as_ref());
                            if let Some(pool) = retry_target {
                                txn.builtin_registry(hub).record_retry(pool);
                            }
                        }
                    }
                    txn.current_phase = next;
                    if phase == Phase::Send {
                        if let Some(hub) = metrics {
                            if hub.metrics_enabled() {
                                record_send_completion_metrics(hub, txn);
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
                cache = %log_text(txn.cache_instance.as_deref().unwrap_or("-")),
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

fn record_send_completion_metrics(hub: &MetricsHub, txn: &Transaction) {
    let protocol = match txn.protocol {
        crate::transaction::ClientProtocol::Udp => "udp",
        crate::transaction::ClientProtocol::Tcp => "tcp",
    };
    let listener = txn.listener_label.as_deref().unwrap_or("unknown");
    let builtin = txn.builtin_registry(hub);
    builtin.record_response(
        listener,
        protocol,
        txn.rcode(),
        &txn.client_addr,
        txn.answer_source.map(|s| s.as_str()),
    );
    if txn.udp_response_truncated_on_send {
        builtin.record_response_truncated(
            listener,
            protocol,
            &txn.client_addr,
            txn.answer_source.map(|s| s.as_str()),
        );
    }
    builtin.observe_response_duration(
        txn.answer_source.map(|s| s.as_str()),
        listener,
        protocol,
        txn.started_at.elapsed().as_secs_f64(),
    );
}

fn phase_name(phase: Phase) -> &'static str {
    match phase {
        Phase::Receive => "receive",
        Phase::Parse => "parse",
        Phase::RequestRules => "request_rules",
        Phase::Lookup => "lookup",
        Phase::NoAnswer => "no_answer",
        Phase::ResponseRules => "response_rules",
        Phase::Send => "send",
        // Internal forward-provider steps — not emitted on the top-level trace path.
        Phase::Route | Phase::Forward | Phase::WaitResponse => "lookup",
    }
}

fn next_phase(phase: Phase) -> Phase {
    match phase {
        Phase::Receive => Phase::Parse,
        Phase::Parse => Phase::RequestRules,
        Phase::RequestRules => Phase::Lookup,
        Phase::Lookup => Phase::ResponseRules,
        Phase::NoAnswer => Phase::Send,
        Phase::ResponseRules => Phase::Send,
        Phase::Send => Phase::Send,
        Phase::Route | Phase::Forward | Phase::WaitResponse => Phase::Lookup,
    }
}

impl Orchestrator {
    pub fn with_default_stages() -> Self {
        use crate::stages::{
            NoAnswerStage, ParseStage, RequestRulesStage, ResponseRulesStage, SendStage,
        };
        let mut registry = StageRegistry::new();
        registry.register(Phase::Parse, Arc::new(ParseStage));
        registry.register(Phase::RequestRules, Arc::new(RequestRulesStage::default()));
        registry.register(Phase::NoAnswer, Arc::new(NoAnswerStage::default()));
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

    use crate::lookup::LookupCacheRegistry;
    use crate::lookup::LookupStage;
    use crate::stages::RouteStage;

    fn fixtures_config_base() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/config")
    }

    fn register_lookup_stage(
        o: &mut Orchestrator,
        forward: Arc<dyn PipelineStage>,
        cache: Option<Arc<LookupCacheRegistry>>,
    ) {
        let mut lookup = LookupStage::new(
            Arc::new(RouteStage::new()),
            forward,
            Arc::new(PassthroughWait),
            None,
        );
        if let Some(c) = cache {
            lookup = lookup.with_cache(c);
        }
        o.registry.register(Phase::Lookup, Arc::new(lookup));
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
        register_lookup_stage(&mut o, Arc::new(MockForwardNoResponse), None);
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
        register_lookup_stage(&mut o, Arc::new(SuspendForwardStage), None);
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
        let _ = orch.resume_after_suspend(&mut txn, &snap, &SystemClock, None, Phase::Lookup);
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
                resume_phase: Phase::Lookup
            }
        );
        assert_eq!(txn.current_phase, Phase::Lookup);
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

        let step = orch.resume_after_suspend(&mut txn, &snap, &SystemClock, None, Phase::Lookup);
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
        let step = orch.resume_after_suspend(&mut txn, &snap, &SystemClock, None, Phase::Lookup);
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
        register_lookup_stage(
            &mut orch,
            Arc::new(SuspendOnceThenRespond {
                calls: AtomicU32::new(0),
            }),
            None,
        );

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

        let step = orch.resume_after_suspend(&mut txn, &snap, &SystemClock, None, Phase::Lookup);
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
        register_lookup_stage(&mut o, Arc::new(MockForwardStage), None);
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
            None,
        );
        assert_eq!(host.tags.has("sampled"), expected);
    }

    use crate::lookup::AnswerSource;
    use std::sync::atomic::{AtomicU32, Ordering};

    struct CountingForward {
        calls: Arc<AtomicU32>,
    }

    impl PipelineStage for CountingForward {
        fn name(&self) -> &'static str {
            "counting_forward"
        }

        fn handle(&self, txn: &mut Transaction, _snapshot: &Arc<RuntimeSnapshot>) -> StageOutcome {
            self.calls.fetch_add(1, Ordering::Relaxed);
            let mut msg = Message::new();
            msg.set_id(txn.dns_id);
            msg.set_response_code(ResponseCode::NoError);
            let mut buf = Vec::new();
            let mut encoder = BinEncoder::new(&mut buf);
            msg.emit(&mut encoder).unwrap();
            txn.response_wire = Some(buf);
            txn.set_rcode(0);
            StageOutcome::Continue(Phase::WaitResponse)
        }
    }

    struct CountingResponseRules {
        calls: Arc<AtomicU32>,
    }

    impl PipelineStage for CountingResponseRules {
        fn name(&self) -> &'static str {
            "counting_response_rules"
        }

        fn handle(&self, txn: &mut Transaction, _snapshot: &Arc<RuntimeSnapshot>) -> StageOutcome {
            let _ = txn;
            self.calls.fetch_add(1, Ordering::Relaxed);
            StageOutcome::Continue(Phase::Send)
        }
    }

    struct ServfailForward {
        calls: Arc<AtomicU32>,
    }

    impl PipelineStage for ServfailForward {
        fn name(&self) -> &'static str {
            "servfail_forward"
        }

        fn handle(&self, txn: &mut Transaction, _snapshot: &Arc<RuntimeSnapshot>) -> StageOutcome {
            self.calls.fetch_add(1, Ordering::Relaxed);
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

    fn orchestrator_with_cache_and_forward(
        forward: Arc<dyn PipelineStage>,
    ) -> (Orchestrator, Arc<RuntimeSnapshot>, Arc<LookupCacheRegistry>) {
        let snap = snapshot_from_fixture(include_str!(
            "../../../tests/fixtures/config/lookup-cache-enabled.yaml"
        ));
        let cache = Arc::new(LookupCacheRegistry::from_snapshot(
            &snap.lookup.cache_instances,
        ));
        let mut orch = Orchestrator::with_default_stages();
        register_lookup_stage(&mut orch, forward, Some(cache.clone()));
        (orch, snap, cache)
    }

    fn snapshot_cache_with_servfail_retry() -> Arc<RuntimeSnapshot> {
        let mut cfg = load_yaml(include_str!(
            "../../../tests/fixtures/config/lookup-cache-enabled.yaml"
        ))
        .unwrap();
        let rules_cfg = load_yaml(include_str!(
            "../../../tests/fixtures/config/with-rules.yaml"
        ))
        .unwrap();
        cfg.rules = rules_cfg.rules;
        cfg.pools = rules_cfg.pools;
        cfg.orchestrator = rules_cfg.orchestrator;
        assert!(conduit_config::validate(&cfg).ok);
        Arc::new(RuntimeSnapshot::from_config_with_base(
            cfg,
            Some(&fixtures_config_base()),
        ))
    }

    fn snapshot_cache_on_hit_skip() -> Arc<RuntimeSnapshot> {
        use conduit_proto::config::CacheOnHitConfig;
        let mut cfg = load_yaml(include_str!(
            "../../../tests/fixtures/config/lookup-cache-enabled.yaml"
        ))
        .unwrap();
        for cache in &mut cfg.caches {
            cache.on_hit = Some(CacheOnHitConfig {
                response_rules: "skip".into(),
            });
        }
        assert!(conduit_config::validate(&cfg).ok);
        Arc::new(RuntimeSnapshot::from_config_with_base(
            cfg,
            Some(&fixtures_config_base()),
        ))
    }

    #[test]
    fn cache_hit_skips_forward() {
        let forward_calls = Arc::new(AtomicU32::new(0));
        let (orch, snap, _cache) = orchestrator_with_cache_and_forward(Arc::new(CountingForward {
            calls: forward_calls.clone(),
        }));

        let mut txn = Transaction::new(1, "127.0.0.1:15353".parse().unwrap(), ClientProtocol::Udp)
            .with_query_wire(example_query());
        let _ = orch.run(&mut txn, &snap, &SystemClock, None);
        assert_eq!(forward_calls.load(Ordering::Relaxed), 1);
        assert_eq!(txn.answer_source, Some(AnswerSource::Forward));

        let mut txn2 = Transaction::new(2, "127.0.0.1:15353".parse().unwrap(), ClientProtocol::Udp)
            .with_query_wire(example_query());
        let _ = orch.run(&mut txn2, &snap, &SystemClock, None);
        assert_eq!(
            forward_calls.load(Ordering::Relaxed),
            1,
            "cache hit must not invoke forward again"
        );
        assert_eq!(txn2.answer_source, Some(AnswerSource::Cache));
        assert_eq!(txn2.cache_instance.as_deref(), Some("global"));
        assert_eq!(
            txn2.rcode(),
            Some(0),
            "cache hit must set RCODE from cached wire"
        );
    }

    #[test]
    fn cache_miss_runs_forward() {
        let forward_calls = Arc::new(AtomicU32::new(0));
        let (orch, snap, _cache) = orchestrator_with_cache_and_forward(Arc::new(CountingForward {
            calls: forward_calls.clone(),
        }));

        let mut txn = Transaction::new(1, "127.0.0.1:15353".parse().unwrap(), ClientProtocol::Udp)
            .with_query_wire(example_query());
        let _ = orch.run(&mut txn, &snap, &SystemClock, None);
        assert_eq!(forward_calls.load(Ordering::Relaxed), 1);
        assert_eq!(
            txn.lookup_outcome,
            Some(crate::lookup::LookupOutcome::Answered)
        );
    }

    #[test]
    fn cache_bypassed_when_lookup_not_eligible() {
        let forward_calls = Arc::new(AtomicU32::new(0));
        let (orch, snap, _cache) = orchestrator_with_cache_and_forward(Arc::new(CountingForward {
            calls: forward_calls.clone(),
        }));

        let mut warm = Transaction::new(1, "127.0.0.1:15353".parse().unwrap(), ClientProtocol::Udp)
            .with_query_wire(example_query());
        let _ = orch.run(&mut warm, &snap, &SystemClock, None);
        assert_eq!(forward_calls.load(Ordering::Relaxed), 1);

        let mut txn = Transaction::new(2, "127.0.0.1:15353".parse().unwrap(), ClientProtocol::Udp)
            .with_query_wire(example_query());
        txn.cache_lookup_eligible = false;
        let _ = orch.run(&mut txn, &snap, &SystemClock, None);
        assert_eq!(
            forward_calls.load(Ordering::Relaxed),
            2,
            "ineligible query must bypass cache and forward"
        );
        assert_eq!(txn.answer_source, Some(AnswerSource::Forward));
    }

    #[test]
    fn retry_reenters_lookup_bypassing_cache() {
        let forward_calls = Arc::new(AtomicU32::new(0));
        let snap = snapshot_cache_with_servfail_retry();
        let cache = Arc::new(LookupCacheRegistry::from_snapshot(
            &snap.lookup.cache_instances,
        ));
        let mut orch = Orchestrator::with_default_stages();
        register_lookup_stage(
            &mut orch,
            Arc::new(ServfailForward {
                calls: forward_calls.clone(),
            }),
            Some(cache),
        );

        let mut txn = Transaction::new(3, "127.0.0.1:15353".parse().unwrap(), ClientProtocol::Udp)
            .with_query_wire(example_query());
        let _ = orch.run(&mut txn, &snap, &SystemClock, None);
        assert!(
            txn.attempts.len() >= 2,
            "SERVFAIL retry should re-enter lookup, attempts={:?}",
            txn.attempts
        );
        assert!(
            forward_calls.load(Ordering::Relaxed) >= 2,
            "retry must run forward again after cache bypass"
        );
        assert_eq!(txn.answer_source, Some(AnswerSource::Forward));
    }

    #[test]
    fn cache_hit_trace_shows_provider_without_route() {
        let forward_calls = Arc::new(AtomicU32::new(0));
        let (orch, snap, _cache) = orchestrator_with_cache_and_forward(Arc::new(CountingForward {
            calls: forward_calls.clone(),
        }));

        let mut cold = Transaction::new(1, "127.0.0.1:15353".parse().unwrap(), ClientProtocol::Udp)
            .with_query_wire(example_query());
        cold.trace_log = Some(conduit_metrics::TraceLog::default());
        let _ = orch.run(&mut cold, &snap, &SystemClock, None);
        assert_eq!(forward_calls.load(Ordering::Relaxed), 1);

        let events = cold.trace_log.as_ref().expect("trace").events.clone();
        let miss = events
            .iter()
            .find(|e| e.message.as_deref() == Some("provider cache miss"))
            .expect("cold miss event");
        assert_eq!(
            miss.cache.as_deref(),
            Some("global"),
            "cache miss must name the instance: {:?}",
            events
        );
        assert!(
            events
                .iter()
                .any(|e| e.message.as_deref() == Some("provider forward answered")),
            "cold miss: {:?}",
            events
        );

        let mut warm = Transaction::new(2, "127.0.0.1:15353".parse().unwrap(), ClientProtocol::Udp)
            .with_query_wire(example_query());
        warm.trace_log = Some(conduit_metrics::TraceLog::default());
        let _ = orch.run(&mut warm, &snap, &SystemClock, None);
        assert_eq!(forward_calls.load(Ordering::Relaxed), 1);

        let warm_events = warm.trace_log.as_ref().expect("trace").events.clone();
        let hit = warm_events
            .iter()
            .find(|e| e.message.as_deref() == Some("provider cache answered"))
            .expect("warm hit event");
        assert_eq!(
            hit.cache.as_deref(),
            Some("global"),
            "cache hit must name the instance: {:?}",
            warm_events
        );
        assert!(
            !warm_events.iter().any(|e| {
                e.message.as_deref() == Some("route selected backend")
                    || e.message.as_deref() == Some("provider forward answered")
            }),
            "cache hit must not record forward route: {:?}",
            warm_events
        );
    }

    #[test]
    fn warm_cache_hit_serves_without_forward() {
        let forward_calls = Arc::new(AtomicU32::new(0));
        let (orch, snap, _cache) = orchestrator_with_cache_and_forward(Arc::new(CountingForward {
            calls: forward_calls.clone(),
        }));

        let mut cold = Transaction::new(1, "127.0.0.1:15353".parse().unwrap(), ClientProtocol::Udp)
            .with_query_wire(example_query());
        let _ = orch.run(&mut cold, &snap, &SystemClock, None);
        assert_eq!(forward_calls.load(Ordering::Relaxed), 1);
        assert_eq!(cold.answer_source, Some(AnswerSource::Forward));

        let mut warm = Transaction::new(2, "127.0.0.1:15353".parse().unwrap(), ClientProtocol::Udp)
            .with_query_wire(example_query());
        let _ = orch.run(&mut warm, &snap, &SystemClock, None);
        assert_eq!(
            forward_calls.load(Ordering::Relaxed),
            1,
            "warm hit must not invoke forward"
        );
        assert_eq!(warm.answer_source, Some(AnswerSource::Cache));
        assert_eq!(warm.rcode(), Some(0));
    }

    #[test]
    fn cache_hit_on_hit_skip_bypasses_response_rules() {
        let forward_calls = Arc::new(AtomicU32::new(0));
        let response_calls = Arc::new(AtomicU32::new(0));
        let snap = snapshot_cache_on_hit_skip();
        assert_eq!(
            snap.lookup.cache_instances["global"].on_hit_response_rules,
            conduit_config::OnHitResponseRules::Skip
        );
        let cache = Arc::new(LookupCacheRegistry::from_snapshot(
            &snap.lookup.cache_instances,
        ));
        let mut orch = Orchestrator::with_default_stages();
        orch.registry.register(
            Phase::ResponseRules,
            Arc::new(CountingResponseRules {
                calls: response_calls.clone(),
            }),
        );
        register_lookup_stage(
            &mut orch,
            Arc::new(CountingForward {
                calls: forward_calls.clone(),
            }),
            Some(cache),
        );

        let mut cold = Transaction::new(1, "127.0.0.1:15353".parse().unwrap(), ClientProtocol::Udp)
            .with_query_wire(example_query());
        let _ = orch.run(&mut cold, &snap, &SystemClock, None);
        assert_eq!(forward_calls.load(Ordering::Relaxed), 1);
        assert_eq!(
            response_calls.load(Ordering::Relaxed),
            1,
            "forward answer still runs response rules when on_hit is skip"
        );
        assert_eq!(cold.answer_source, Some(AnswerSource::Forward));

        let mut warm = Transaction::new(2, "127.0.0.1:15353".parse().unwrap(), ClientProtocol::Udp)
            .with_query_wire(example_query());
        let _ = orch.run(&mut warm, &snap, &SystemClock, None);
        assert_eq!(forward_calls.load(Ordering::Relaxed), 1);
        assert_eq!(
            response_calls.load(Ordering::Relaxed),
            1,
            "cache hit with on_hit.response_rules=skip must not enter ResponseRules"
        );
        assert_eq!(warm.answer_source, Some(AnswerSource::Cache));
        assert_eq!(warm.cache_instance.as_deref(), Some("global"));
    }
}
