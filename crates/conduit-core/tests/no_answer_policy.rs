//! NoAnswer policy hook and route_failure_policy (Phase B).

use conduit_config::{load_yaml, validate};
use conduit_core::clock::SystemClock;
use conduit_core::lookup::LookupStage;
use conduit_core::orchestrator::Orchestrator;
use conduit_core::phase::Phase;
use conduit_core::pipeline::{PipelineStage, StageOutcome};
use conduit_core::snapshot::RuntimeSnapshot;
use conduit_core::stages::{RouteStage, SendStage};
use conduit_core::transaction::{ClientProtocol, ConvergenceReason, Transaction};
use conduit_core::RunOutcome;
use hickory_proto::op::{Message, Query};
use hickory_proto::rr::{Name, RecordType};
use hickory_proto::serialize::binary::{BinEncodable, BinEncoder};
use std::net::SocketAddr;
use std::sync::Arc;

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

fn run_outcome(
    orch: &Orchestrator,
    snap: &Arc<RuntimeSnapshot>,
    mut txn: Transaction,
) -> (RunOutcome, Transaction) {
    let outcome = orch.run(&mut txn, snap, &SystemClock, None);
    (outcome, txn)
}

fn base_yaml(orchestrator_extra: &str, rules_extra: &str) -> String {
    format!(
        r#"
schema_version: 1
listeners:
  listeners:
    - address: "127.0.0.1:15353"
      protocol: udp
orchestrator:
  max_attempts: 3
  {orchestrator_extra}
pools:
  - name: primary
    backends:
      - address: "127.0.0.1:5300"
        weight: 100
{rules_extra}
"#
    )
}

#[test]
fn no_answer_rule_overrides_rcode() {
    let yaml = base_yaml(
        "",
        r#"
rules:
  match_mode: first_match
  rules:
    - name: refuse-total-failure
      hook: no_answer
      selectors: []
      actions:
        - type: set_rcode
          value: REFUSED
"#,
    );
    let cfg = load_yaml(&yaml).unwrap();
    assert!(validate(&cfg).ok);
    let snap = Arc::new(RuntimeSnapshot::from_config(cfg));
    let mut orch = Orchestrator::with_default_stages();
    struct FailRoute;
    impl PipelineStage for FailRoute {
        fn name(&self) -> &'static str {
            "fail_route"
        }
        fn handle(&self, txn: &mut Transaction, _snapshot: &Arc<RuntimeSnapshot>) -> StageOutcome {
            txn.set_rcode_name("SERVFAIL");
            txn.set_convergence_reason(ConvergenceReason::NoBackendSelected);
            StageOutcome::Continue(Phase::NoAnswer)
        }
    }
    register_lookup(&mut orch, Arc::new(FailRoute));
    orch.registry.register(Phase::Send, Arc::new(SendStage));

    let mut txn = Transaction::new(
        1,
        "127.0.0.1:53".parse::<SocketAddr>().unwrap(),
        ClientProtocol::Udp,
    );
    txn.query_wire = query_wire();
    let (outcome, txn) = run_outcome(&orch, &snap, txn);
    assert!(matches!(outcome, RunOutcome::Response(_)));
    assert_eq!(txn.rcode_label().as_deref(), Some("REFUSED"));
}

#[test]
fn no_answer_rule_silent_drop() {
    let yaml = base_yaml(
        "",
        r#"
rules:
  match_mode: first_match
  rules:
    - name: drop-total-failure
      hook: no_answer
      selectors: []
      actions:
        - type: drop
"#,
    );
    let cfg = load_yaml(&yaml).unwrap();
    let snap = Arc::new(RuntimeSnapshot::from_config(cfg));
    let mut orch = Orchestrator::with_default_stages();
    struct FailRoute;
    impl PipelineStage for FailRoute {
        fn name(&self) -> &'static str {
            "fail_route"
        }
        fn handle(&self, txn: &mut Transaction, _snapshot: &Arc<RuntimeSnapshot>) -> StageOutcome {
            txn.set_convergence_reason(ConvergenceReason::NoPool);
            StageOutcome::Continue(Phase::NoAnswer)
        }
    }
    register_lookup(&mut orch, Arc::new(FailRoute));
    orch.registry.register(Phase::Send, Arc::new(SendStage));

    let mut txn = Transaction::new(
        1,
        "127.0.0.1:53".parse::<SocketAddr>().unwrap(),
        ClientProtocol::Udp,
    );
    txn.query_wire = query_wire();
    let (outcome, _) = run_outcome(&orch, &snap, txn);
    assert!(matches!(outcome, RunOutcome::Dropped));
}

#[test]
fn no_answer_retry_rejected_at_validate() {
    let yaml = base_yaml(
        "",
        r#"
rules:
  match_mode: first_match
  rules:
    - name: bad-retry
      hook: no_answer
      selectors: []
      actions:
        - type: retry
"#,
    );
    let cfg = load_yaml(&yaml).unwrap();
    let report = validate(&cfg);
    assert!(!report.ok);
    assert!(
        report
            .errors
            .iter()
            .any(|e| e.contains("no_answer") && e.contains("retry")),
        "errors: {:?}",
        report.errors
    );
}

#[test]
fn unknown_hook_rejected_at_validate() {
    let yaml = base_yaml(
        "",
        r#"
rules:
  match_mode: first_match
  rules:
    - name: typo
      hook: no_answr
      selectors: []
      actions:
        - type: drop
"#,
    );
    let cfg = load_yaml(&yaml).unwrap();
    let report = validate(&cfg);
    assert!(!report.ok);
    assert!(
        report.errors.iter().any(|e| e.contains("no_answr")),
        "errors: {:?}",
        report.errors
    );
}

#[test]
fn no_answer_answer_source_selector_does_not_match() {
    let yaml = base_yaml(
        "",
        r#"
rules:
  match_mode: first_match
  rules:
    - name: cache-only
      hook: no_answer
      selectors:
        - type: answer_source
          value: cache
      actions:
        - type: set_rcode
          value: REFUSED
"#,
    );
    let cfg = load_yaml(&yaml).unwrap();
    let snap = Arc::new(RuntimeSnapshot::from_config(cfg));
    let mut orch = Orchestrator::with_default_stages();
    struct FailRoute;
    impl PipelineStage for FailRoute {
        fn name(&self) -> &'static str {
            "fail_route"
        }
        fn handle(&self, txn: &mut Transaction, _snapshot: &Arc<RuntimeSnapshot>) -> StageOutcome {
            txn.set_rcode_name("SERVFAIL");
            txn.set_convergence_reason(ConvergenceReason::NoBackendSelected);
            StageOutcome::Continue(Phase::NoAnswer)
        }
    }
    register_lookup(&mut orch, Arc::new(FailRoute));
    orch.registry.register(Phase::Send, Arc::new(SendStage));

    let mut txn = Transaction::new(
        1,
        "127.0.0.1:53".parse::<SocketAddr>().unwrap(),
        ClientProtocol::Udp,
    );
    txn.query_wire = query_wire();
    let (outcome, txn) = run_outcome(&orch, &snap, txn);
    assert!(matches!(outcome, RunOutcome::Response(_)));
    assert_eq!(txn.rcode_label().as_deref(), Some("SERVFAIL"));
}

#[test]
fn default_route_failure_policy_skips_response_rules() {
    let yaml = r#"
schema_version: 1
listeners:
  listeners:
    - address: "127.0.0.1:15353"
      protocol: udp
pools:
  - name: primary
    backends: []
rules:
  match_mode: first_match
  rules:
    - name: would-tag
      hook: response
      selectors: []
      actions:
        - type: set_tag
          value: "seen=true"
"#;
    let cfg = load_yaml(yaml).unwrap();
    let snap = Arc::new(RuntimeSnapshot::from_config(cfg));
    let mut orch = Orchestrator::with_default_stages();
    struct NeverForward;
    impl PipelineStage for NeverForward {
        fn name(&self) -> &'static str {
            "forward"
        }
        fn handle(&self, _txn: &mut Transaction, _snapshot: &Arc<RuntimeSnapshot>) -> StageOutcome {
            unreachable!("route failure should stop before forward")
        }
    }
    orch.registry.register(
        Phase::Lookup,
        Arc::new(LookupStage::new(
            Arc::new(RouteStage::new()),
            Arc::new(NeverForward),
            Arc::new(PassthroughWaitStub),
            None,
        )),
    );
    orch.registry.register(Phase::Send, Arc::new(SendStage));

    let mut txn = Transaction::new(
        1,
        "127.0.0.1:53".parse::<SocketAddr>().unwrap(),
        ClientProtocol::Udp,
    );
    txn.query_wire = query_wire();
    txn.selected_pool = Some("primary".into());
    let (outcome, txn) = run_outcome(&orch, &snap, txn);
    assert!(matches!(outcome, RunOutcome::Response(_)));
    assert!(
        !txn.tags.has("seen"),
        "response rules must not run on default policy"
    );
}

struct PassthroughWaitStub;
impl PipelineStage for PassthroughWaitStub {
    fn name(&self) -> &'static str {
        "wait"
    }
    fn handle(&self, _txn: &mut Transaction, _snapshot: &Arc<RuntimeSnapshot>) -> StageOutcome {
        StageOutcome::Continue(Phase::ResponseRules)
    }
}

#[test]
fn response_rules_policy_failover_on_route_failure() {
    let yaml = r#"
schema_version: 1
listeners:
  listeners:
    - address: "127.0.0.1:15353"
      protocol: udp
orchestrator:
  route_failure_policy: response_rules
  max_attempts: 3
pools:
  - name: secondary
    backends:
      - address: "127.0.0.1:5301"
        weight: 100
rules:
  match_mode: first_match
  rules:
    - name: failover
      hook: response
      selectors: []
      actions:
        - type: set_retry_pool
          value: secondary
        - type: retry
"#;
    let cfg = load_yaml(yaml).unwrap();
    assert!(validate(&cfg).ok);
    let snap = Arc::new(RuntimeSnapshot::from_config(cfg));
    let mut orch = Orchestrator::with_default_stages();

    struct MockForward;
    impl PipelineStage for MockForward {
        fn name(&self) -> &'static str {
            "mock_forward"
        }
        fn handle(&self, txn: &mut Transaction, _snapshot: &Arc<RuntimeSnapshot>) -> StageOutcome {
            let mut msg = Message::new();
            msg.set_id(txn.dns_id);
            msg.set_response_code(hickory_proto::op::ResponseCode::NoError);
            let mut buf = Vec::new();
            let mut encoder = BinEncoder::new(&mut buf);
            msg.emit(&mut encoder).unwrap();
            txn.response_wire = Some(buf);
            txn.set_rcode(0);
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
    orch.registry.register(
        Phase::Lookup,
        Arc::new(LookupStage::new(
            Arc::new(RouteStage::new()),
            Arc::new(MockForward),
            Arc::new(PassthroughWait),
            None,
        )),
    );
    orch.registry.register(Phase::Send, Arc::new(SendStage));

    let mut txn = Transaction::new(
        1,
        "127.0.0.1:53".parse::<SocketAddr>().unwrap(),
        ClientProtocol::Udp,
    );
    txn.query_wire = query_wire();
    txn.selected_pool = Some("missing".into());
    let (outcome, txn) = run_outcome(&orch, &snap, txn);
    assert!(
        matches!(outcome, RunOutcome::Response(_)),
        "failover should answer"
    );
    assert_eq!(txn.selected_pool.as_deref(), Some("secondary"));
}

#[test]
fn response_rules_policy_declined_retry_converges_at_no_answer() {
    let yaml = r#"
schema_version: 1
listeners:
  listeners:
    - address: "127.0.0.1:15353"
      protocol: udp
orchestrator:
  route_failure_policy: response_rules
pools:
  - name: primary
    backends: []
rules:
  match_mode: first_match
  rules:
    - name: no-op
      hook: response
      selectors: []
      actions:
        - type: set_tag
          value: "seen=true"
"#;
    let cfg = load_yaml(yaml).unwrap();
    let snap = Arc::new(RuntimeSnapshot::from_config(cfg));
    let mut orch = Orchestrator::with_default_stages();
    orch.registry.register(
        Phase::Lookup,
        Arc::new(LookupStage::new(
            Arc::new(RouteStage::new()),
            Arc::new(NeverForward),
            Arc::new(PassthroughWaitStub),
            None,
        )),
    );
    orch.registry.register(Phase::Send, Arc::new(SendStage));

    let mut txn = Transaction::new(
        1,
        "127.0.0.1:53".parse::<SocketAddr>().unwrap(),
        ClientProtocol::Udp,
    );
    txn.query_wire = query_wire();
    txn.selected_pool = Some("primary".into());
    let (outcome, txn) = run_outcome(&orch, &snap, txn);
    assert!(matches!(outcome, RunOutcome::Response(_)));
    assert!(txn.tags.has("seen"));
    assert_eq!(
        txn.convergence_reason,
        Some(ConvergenceReason::NoBackendSelected)
    );
    assert_eq!(txn.rcode_label().as_deref(), Some("SERVFAIL"));
}

struct NeverForward;
impl PipelineStage for NeverForward {
    fn name(&self) -> &'static str {
        "forward"
    }
    fn handle(&self, _txn: &mut Transaction, _snapshot: &Arc<RuntimeSnapshot>) -> StageOutcome {
        unreachable!("route failure stops before forward")
    }
}
