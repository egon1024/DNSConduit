//! Failure convergence spine: NoAnswer phase preserves pre-change wire and appears in traces.

use conduit_config::load_yaml;
use conduit_core::clock::SystemClock;
use conduit_core::lookup::LookupStage;
use conduit_core::orchestrator::Orchestrator;
use conduit_core::phase::Phase;
use conduit_core::pipeline::{PipelineStage, StageOutcome};
use conduit_core::snapshot::RuntimeSnapshot;
use conduit_core::stages::{build_error_response, RouteStage, SendStage};
use conduit_core::transaction::{ClientProtocol, ConvergenceReason, Transaction};
use conduit_core::{OrchestratorRun, RunOutcome};
use hickory_proto::op::{Message, Query};
use hickory_proto::rr::{Name, RecordType};
use hickory_proto::serialize::binary::{BinEncodable, BinEncoder};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

fn fixtures_config_base() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/config")
}

fn query_wire() -> Vec<u8> {
    let name = Name::from_utf8("example.com.").unwrap();
    let query = Query::query(name, RecordType::A);
    let mut msg = Message::new();
    msg.set_id(0xabcd);
    msg.add_query(query);
    let mut buf = Vec::new();
    let mut encoder = BinEncoder::new(&mut buf);
    msg.emit(&mut encoder).unwrap();
    buf
}

fn expected_servfail_wire(txn: &Transaction) -> Vec<u8> {
    let (wire, _, _) = build_error_response(
        txn.dns_id,
        txn.rcode().unwrap_or(2),
        &txn.query_wire,
        txn.client_udp_payload_size,
    );
    wire
}

fn register_lookup(o: &mut Orchestrator, forward: Arc<dyn PipelineStage>) {
    struct PassthroughWait;
    impl PipelineStage for PassthroughWait {
        fn name(&self) -> &'static str {
            "wait"
        }
        fn handle(&self, _txn: &mut Transaction, _snapshot: &Arc<RuntimeSnapshot>) -> StageOutcome {
            StageOutcome::Continue(Phase::ResponseRules)
        }
    }
    let lookup = LookupStage::new(
        Arc::new(RouteStage::new()),
        forward,
        Arc::new(PassthroughWait),
        None,
    );
    o.registry.register(Phase::Lookup, Arc::new(lookup));
}

fn run_to_response(
    orch: &Orchestrator,
    snap: &Arc<RuntimeSnapshot>,
    mut txn: Transaction,
) -> Transaction {
    match orch.run(&mut txn, snap, &SystemClock, None) {
        RunOutcome::Response(_) => txn,
        other => panic!("expected Response, got {other:?}"),
    }
}

fn assert_byte_identical_servfail(txn: &Transaction) {
    let got = txn.response_wire.as_ref().expect("response wire");
    let expected = expected_servfail_wire(txn);
    assert_eq!(
        got, &expected,
        "client wire must match pre-change synthesized SERVFAIL"
    );
    assert_eq!(txn.rcode_label().as_deref(), Some("SERVFAIL"));
}

#[test]
fn no_pool_converges_byte_identical() {
    let yaml = r#"
schema_version: 1
listeners:
  listeners:
    - address: "127.0.0.1:15353"
      protocol: udp
pools: []
"#;
    let cfg = load_yaml(yaml).unwrap();
    let snap = Arc::new(RuntimeSnapshot::from_config(cfg));
    let mut orch = Orchestrator::with_default_stages();
    struct NeverForward;
    impl PipelineStage for NeverForward {
        fn name(&self) -> &'static str {
            "never"
        }
        fn handle(&self, _txn: &mut Transaction, _snapshot: &Arc<RuntimeSnapshot>) -> StageOutcome {
            panic!("forward must not run when routing fails");
        }
    }
    register_lookup(&mut orch, Arc::new(NeverForward));

    let mut txn = Transaction::new(
        1,
        "127.0.0.1:53".parse::<SocketAddr>().unwrap(),
        ClientProtocol::Udp,
    )
    .with_query_wire(query_wire());
    txn = run_to_response(&orch, &snap, txn);
    assert_eq!(txn.convergence_reason, Some(ConvergenceReason::NoPool));
    assert_byte_identical_servfail(&txn);
}

#[test]
fn no_backend_selected_converges_byte_identical() {
    let yaml = r#"
schema_version: 1
listeners:
  listeners:
    - address: "127.0.0.1:15353"
      protocol: udp
pools:
  - name: default
    backends:
      - address: "127.0.0.1:5300"
        weight: 100
"#;
    let cfg = load_yaml(yaml).unwrap();
    let snap = Arc::new(RuntimeSnapshot::from_config(cfg));
    let mut orch = Orchestrator::with_default_stages();
    struct NeverForward;
    impl PipelineStage for NeverForward {
        fn name(&self) -> &'static str {
            "never"
        }
        fn handle(&self, _txn: &mut Transaction, _snapshot: &Arc<RuntimeSnapshot>) -> StageOutcome {
            panic!("forward must not run when no backend selectable");
        }
    }
    register_lookup(&mut orch, Arc::new(NeverForward));

    let mut txn = Transaction::new(2, "127.0.0.1:53".parse().unwrap(), ClientProtocol::Udp)
        .with_query_wire(query_wire());
    // Named pool that does not exist → selection fails after pool name is resolved.
    txn.selected_pool = Some("missing-pool".into());
    txn = run_to_response(&orch, &snap, txn);
    assert_eq!(
        txn.convergence_reason,
        Some(ConvergenceReason::NoBackendSelected)
    );
    assert_byte_identical_servfail(&txn);
}

#[test]
fn attempts_exhausted_converges_byte_identical() {
    let yaml = include_str!("../../../tests/fixtures/config/minimal.yaml");
    let mut cfg = load_yaml(yaml).unwrap();
    cfg.orchestrator.as_mut().unwrap().max_attempts = 1;
    let snap = Arc::new(RuntimeSnapshot::from_config_with_base(
        cfg,
        Some(&fixtures_config_base()),
    ));
    let mut orch = Orchestrator::with_default_stages();
    struct NeverForward;
    impl PipelineStage for NeverForward {
        fn name(&self) -> &'static str {
            "never"
        }
        fn handle(&self, _txn: &mut Transaction, _snapshot: &Arc<RuntimeSnapshot>) -> StageOutcome {
            panic!("forward must not run when attempts already exhausted");
        }
    }
    register_lookup(&mut orch, Arc::new(NeverForward));

    let mut txn = Transaction::new(3, "127.0.0.1:53".parse().unwrap(), ClientProtocol::Udp)
        .with_query_wire(query_wire());
    txn.attempt_count = 1;
    txn = run_to_response(&orch, &snap, txn);
    assert_eq!(
        txn.convergence_reason,
        Some(ConvergenceReason::AttemptsExhausted)
    );
    assert_byte_identical_servfail(&txn);
}

#[test]
fn unknown_profile_converges_byte_identical() {
    let yaml = include_str!("../../../tests/fixtures/config/minimal.yaml");
    let cfg = load_yaml(yaml).unwrap();
    let snap = Arc::new(RuntimeSnapshot::from_config_with_base(
        cfg,
        Some(&fixtures_config_base()),
    ));
    let mut orch = Orchestrator::with_default_stages();
    struct NeverForward;
    impl PipelineStage for NeverForward {
        fn name(&self) -> &'static str {
            "never"
        }
        fn handle(&self, _txn: &mut Transaction, _snapshot: &Arc<RuntimeSnapshot>) -> StageOutcome {
            panic!("forward must not run for unknown profile");
        }
    }
    register_lookup(&mut orch, Arc::new(NeverForward));

    let mut txn = Transaction::new(4, "127.0.0.1:53".parse().unwrap(), ClientProtocol::Udp)
        .with_query_wire(query_wire());
    txn.lookup_profile = Some("does-not-exist".into());
    txn = run_to_response(&orch, &snap, txn);
    assert_eq!(
        txn.convergence_reason,
        Some(ConvergenceReason::UnknownProfile)
    );
    assert_byte_identical_servfail(&txn);
}

#[test]
fn forward_error_converges_byte_identical() {
    let yaml = include_str!("../../../tests/fixtures/config/minimal.yaml");
    let cfg = load_yaml(yaml).unwrap();
    let snap = Arc::new(RuntimeSnapshot::from_config_with_base(
        cfg,
        Some(&fixtures_config_base()),
    ));
    let mut orch = Orchestrator::with_default_stages();
    struct HardFailForward;
    impl PipelineStage for HardFailForward {
        fn name(&self) -> &'static str {
            "hard_fail"
        }
        fn handle(&self, txn: &mut Transaction, _snapshot: &Arc<RuntimeSnapshot>) -> StageOutcome {
            txn.set_rcode_name("SERVFAIL");
            txn.set_convergence_reason(ConvergenceReason::ForwardError);
            StageOutcome::Continue(Phase::NoAnswer)
        }
    }
    register_lookup(&mut orch, Arc::new(HardFailForward));

    let mut txn = Transaction::new(5, "127.0.0.1:53".parse().unwrap(), ClientProtocol::Udp)
        .with_query_wire(query_wire());
    txn = run_to_response(&orch, &snap, txn);
    assert_eq!(
        txn.convergence_reason,
        Some(ConvergenceReason::ForwardError)
    );
    assert_byte_identical_servfail(&txn);
}

#[test]
fn duration_exhausted_converges_byte_identical() {
    let yaml = include_str!("../../../tests/fixtures/config/minimal.yaml");
    let mut cfg = load_yaml(yaml).unwrap();
    cfg.orchestrator.as_mut().unwrap().max_txn_duration_ms = 1;
    let snap = Arc::new(RuntimeSnapshot::from_config_with_base(
        cfg,
        Some(&fixtures_config_base()),
    ));
    let mut orch = Orchestrator::with_default_stages();
    struct SlowForward;
    impl PipelineStage for SlowForward {
        fn name(&self) -> &'static str {
            "slow"
        }
        fn handle(&self, _txn: &mut Transaction, _snapshot: &Arc<RuntimeSnapshot>) -> StageOutcome {
            std::thread::sleep(Duration::from_millis(20));
            StageOutcome::Suspend(Phase::WaitResponse)
        }
    }
    register_lookup(&mut orch, Arc::new(SlowForward));

    let mut txn = Transaction::new(6, "127.0.0.1:53".parse().unwrap(), ClientProtocol::Udp)
        .with_query_wire(query_wire());
    txn.started_at = Instant::now() - Duration::from_millis(50);
    match orch.run_until_suspend(&mut txn, &snap, &SystemClock, None) {
        OrchestratorRun::Suspended { .. } => {}
        OrchestratorRun::Finished(RunOutcome::Response(_)) => {
            assert_eq!(
                txn.convergence_reason,
                Some(ConvergenceReason::DurationExhausted)
            );
            assert_byte_identical_servfail(&txn);
            return;
        }
        other => panic!("unexpected {other:?}"),
    }
    let step = orch.resume_after_suspend(&mut txn, &snap, &SystemClock, None, Phase::Lookup);
    assert!(matches!(
        step,
        OrchestratorRun::Finished(RunOutcome::Response(_))
    ));
    assert_eq!(
        txn.convergence_reason,
        Some(ConvergenceReason::DurationExhausted)
    );
    assert_byte_identical_servfail(&txn);
}

#[test]
fn stale_miss_reason_preserves_servfail_wire_shape() {
    let mut txn = Transaction::new(7, "127.0.0.1:53".parse().unwrap(), ClientProtocol::Udp)
        .with_query_wire(query_wire());
    txn.dns_id = 0xabcd;
    txn.set_rcode_name("SERVFAIL");
    txn.set_convergence_reason(ConvergenceReason::StaleMiss);
    assert_eq!(ConvergenceReason::StaleMiss.as_str(), "stale_miss");

    let snap = Arc::new(RuntimeSnapshot::from_config(
        load_yaml(include_str!("../../../tests/fixtures/config/minimal.yaml")).unwrap(),
    ));
    let _ = SendStage.handle(&mut txn, &snap);
    assert_byte_identical_servfail(&txn);
}

#[test]
fn no_answer_appears_in_trace_and_existing_phases_unchanged() {
    let yaml = r#"
schema_version: 1
listeners:
  listeners:
    - address: "127.0.0.1:15353"
      protocol: udp
pools: []
"#;
    let cfg = load_yaml(yaml).unwrap();
    let snap = Arc::new(RuntimeSnapshot::from_config(cfg));
    let mut orch = Orchestrator::with_default_stages();
    struct NeverForward;
    impl PipelineStage for NeverForward {
        fn name(&self) -> &'static str {
            "never"
        }
        fn handle(&self, _txn: &mut Transaction, _snapshot: &Arc<RuntimeSnapshot>) -> StageOutcome {
            panic!("unreachable");
        }
    }
    register_lookup(&mut orch, Arc::new(NeverForward));

    let mut txn = Transaction::new(8, "127.0.0.1:53".parse().unwrap(), ClientProtocol::Udp)
        .with_query_wire(query_wire());
    txn.trace_log = Some(conduit_metrics::TraceLog::default());
    txn = run_to_response(&orch, &snap, txn);

    let phases: Vec<&str> = txn
        .trace_log
        .as_ref()
        .expect("trace")
        .events
        .iter()
        .map(|e| e.phase.as_str())
        .collect();
    assert!(
        phases.contains(&"no_answer"),
        "expected no_answer in {phases:?}"
    );
    for expected in ["parse", "request_rules", "lookup", "send"] {
        assert!(
            phases.contains(&expected),
            "missing existing phase {expected} in {phases:?}"
        );
    }
    assert!(
        !phases
            .iter()
            .any(|p| *p == "route" || *p == "forward" || *p == "wait_response"),
        "legacy internal phases must not appear as top-level: {phases:?}"
    );
    assert!(
        !phases.contains(&"response_rules"),
        "response_rules must be skipped on no_pool convergence: {phases:?}"
    );
}
