//! Phase state machine driving pipeline stages (spec §3.1).

use crate::clock::Clock;
use crate::observation_emit::{emit_query, emit_response, emit_retry};
use crate::phase::Phase;
use crate::pipeline::{PipelineStage, StageOutcome};
use crate::snapshot::RuntimeSnapshot;
use crate::transaction::Transaction;
use conduit_observation::ObservationHub;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

#[derive(Debug, PartialEq, Eq)]
pub enum RunOutcome {
    Response(Vec<u8>),
    Dropped,
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
}

impl Orchestrator {
    pub fn run(
        &self,
        txn: &mut Transaction,
        snapshot: &Arc<RuntimeSnapshot>,
        _clock: &dyn Clock,
        observation: Option<&ObservationHub>,
    ) -> RunOutcome {
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

        txn.snapshot_generation = snapshot.generation;
        txn.current_phase = Phase::Parse;

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
                    if let Some(hub) = observation {
                        emit_response(hub, txn, snapshot);
                        emit_retry(hub, txn, snapshot);
                    }
                    break;
                }
                txn.current_phase = next_phase(txn.current_phase);
                continue;
            };

            let phase = txn.current_phase;
            let outcome = stage.handle(txn, snapshot);
            match outcome {
                StageOutcome::Drop => return RunOutcome::Dropped,
                StageOutcome::Continue(next) => {
                    if phase == Phase::RequestRules && txn.qname.is_some() {
                        if let Some(hub) = observation {
                            emit_query(hub, txn, snapshot);
                        }
                    }
                    txn.current_phase = next;
                    if txn.current_phase == Phase::Send {
                        if let Some(hub) = observation {
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

        match &outcome {
            RunOutcome::Response(_) => tracing::info!(
                txn_id = txn.id,
                dns_id = txn.dns_id,
                qname = ?txn.qname,
                rcode = ?txn.rcode_label(),
                pool = ?txn.selected_pool,
                backend = ?txn.selected_backend,
                attempts = txn.attempt_count,
                "query complete"
            ),
            RunOutcome::Dropped => tracing::warn!(
                txn_id = txn.id,
                dns_id = txn.dns_id,
                qname = ?txn.qname,
                "query dropped"
            ),
        }

        outcome
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
        registry.register(Phase::RequestRules, Arc::new(RequestRulesStage));
        registry.register(Phase::Route, Arc::new(RouteStage));
        registry.register(Phase::ResponseRules, Arc::new(ResponseRulesStage));
        registry.register(Phase::Send, Arc::new(SendStage));
        Self { registry }
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
        let mut txn = Transaction::new(1, "127.0.0.1:5353".parse().unwrap(), ClientProtocol::Udp)
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
        let mut txn = Transaction::new(2, "127.0.0.1:5353".parse().unwrap(), ClientProtocol::Udp)
            .with_query_wire(example_query());
        let orch = orchestrator_with_mock_forward();
        let _ = orch.run(&mut txn, &snap, &SystemClock, None);
        assert!(txn.attempts.len() >= 2);
        assert_eq!(txn.attempts[0].pool, "primary");
        assert_eq!(txn.attempts.last().unwrap().pool, "secondary");
    }

    #[test]
    fn query_observation_after_request_rules_respects_tag_required() {
        use conduit_observation::ObservationHub;

        let yaml = include_str!("../../../tests/fixtures/config/with-dnstap-filters.yaml");
        let cfg = load_yaml(yaml).unwrap();
        let snap = Arc::new(RuntimeSnapshot::from_config(cfg));
        let hub = ObservationHub::from_compiled(&snap.observation);
        let orch = orchestrator_with_mock_forward();

        let name = Name::from_utf8("pay.payments.corp.example.").unwrap();
        let query = Query::query(name, RecordType::A);
        let mut msg = Message::new();
        msg.add_query(query);
        let mut buf = Vec::new();
        let mut encoder = BinEncoder::new(&mut buf);
        msg.emit(&mut encoder).unwrap();

        let mut txn = Transaction::new(10, "127.0.0.1:5353".parse().unwrap(), ClientProtocol::Udp)
            .with_query_wire(buf);
        let _ = orch.run(&mut txn, &snap, &SystemClock, Some(&hub));
        let metrics = hub.sink_metrics_snapshot();
        assert!(
            metrics[0].enqueued_query >= 1,
            "tag set in request rules should allow query export"
        );

        let mut txn2 = Transaction::new(11, "127.0.0.1:5353".parse().unwrap(), ClientProtocol::Udp)
            .with_query_wire(example_query());
        let _ = orch.run(&mut txn2, &snap, &SystemClock, Some(&hub));
        let metrics2 = hub.sink_metrics_snapshot();
        assert_eq!(
            metrics2[0].enqueued_query, metrics[0].enqueued_query,
            "query without audit tag should not enqueue"
        );
    }
}
