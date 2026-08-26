//! Per-query lookup profile selection (request rules and request-phase Rhai).

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

fn tag_string(txn: &Transaction, key: &str) -> Option<String> {
    let (_, strings) = txn.tags.export_all_tags();
    strings.into_iter().find(|(k, _)| k == key).map(|(_, v)| v)
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

/// Records the active profile on each forward; avoids process-global counters so
/// parallel cargo tests cannot race.
struct ProfileRecordingForward;
impl PipelineStage for ProfileRecordingForward {
    fn name(&self) -> &'static str {
        "forward"
    }

    fn handle(&self, txn: &mut Transaction, _snapshot: &Arc<RuntimeSnapshot>) -> StageOutcome {
        let profile = txn.lookup_profile_name().to_string();
        txn.tags.set_string("lookup_profile_at_forward", profile);
        // Count forwards on the txn so retries are observable without shared state.
        let prev = tag_string(txn, "forward_hits")
            .and_then(|s| s.parse::<u32>().ok())
            .unwrap_or(0);
        txn.tags.set_string("forward_hits", (prev + 1).to_string());
        txn.set_rcode_name("NOERROR");
        txn.response_wire = Some(txn.query_wire.clone());
        StageOutcome::Continue(Phase::ResponseRules)
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

fn register_lookup(o: &mut Orchestrator) {
    let lookup = LookupStage::new(
        Arc::new(RouteStage::new()),
        Arc::new(ProfileRecordingForward),
        Arc::new(PassthroughWait),
        None,
    );
    o.registry.register(Phase::Lookup, Arc::new(lookup));
}

fn two_profile_yaml(max_attempts: u32, rules_extra: &str) -> String {
    format!(
        r#"
schema_version: 1
listeners:
  listeners:
    - address: "127.0.0.1:15353"
      protocol: udp
orchestrator:
  max_attempts: {max_attempts}
pools:
  - name: default
    backends:
      - address: "127.0.0.1:5300"
        weight: 100
lookup:
  profiles:
    default:
      providers:
        - type: forward
    secondary:
      providers:
        - type: forward
{rules_extra}
"#
    )
}

fn run_query(snap: &Arc<RuntimeSnapshot>, orch: &Orchestrator) -> Transaction {
    let mut txn = Transaction::new(
        1,
        "127.0.0.1:53".parse::<SocketAddr>().unwrap(),
        ClientProtocol::Udp,
    );
    txn.query_wire = query_wire();
    match orch.run(&mut txn, snap, &SystemClock, None) {
        RunOutcome::Response(_) | RunOutcome::Dropped => txn,
    }
}

fn forward_hits(txn: &Transaction) -> u32 {
    tag_string(txn, "forward_hits")
        .and_then(|s| s.parse().ok())
        .unwrap_or(0)
}

#[test]
fn request_rule_selects_lookup_profile() {
    let yaml = two_profile_yaml(
        3,
        r#"
rules:
  match_mode: first_match
  rules:
    - name: pick-secondary
      hook: request
      selectors: []
      actions:
        - type: set_lookup_profile
          value: secondary
"#,
    );
    let cfg = load_yaml(&yaml).unwrap();
    assert!(validate(&cfg).ok, "{:?}", validate(&cfg).errors);
    let snap = Arc::new(RuntimeSnapshot::from_config(cfg));
    let mut orch = Orchestrator::with_default_stages();
    register_lookup(&mut orch);
    orch.registry.register(Phase::Send, Arc::new(SendStage));

    let txn = run_query(&snap, &orch);
    assert_eq!(
        tag_string(&txn, "lookup_profile_at_forward").as_deref(),
        Some("secondary")
    );
}

#[test]
fn default_profile_when_unset() {
    let yaml = two_profile_yaml(3, "");
    let cfg = load_yaml(&yaml).unwrap();
    let snap = Arc::new(RuntimeSnapshot::from_config(cfg));
    let mut orch = Orchestrator::with_default_stages();
    register_lookup(&mut orch);
    orch.registry.register(Phase::Send, Arc::new(SendStage));

    let txn = run_query(&snap, &orch);
    assert_eq!(
        tag_string(&txn, "lookup_profile_at_forward").as_deref(),
        Some("default")
    );
}

#[test]
fn profile_fixed_across_response_rule_retry() {
    // Two backends so a response-rule retry can select a different one (Route
    // excludes already-tried backends in the same pool).
    let yaml = r#"
schema_version: 1
listeners:
  listeners:
    - address: "127.0.0.1:15353"
      protocol: udp
orchestrator:
  max_attempts: 2
pools:
  - name: default
    backends:
      - address: "127.0.0.1:5300"
        weight: 100
      - address: "127.0.0.1:5301"
        weight: 100
lookup:
  profiles:
    default:
      providers:
        - type: forward
    secondary:
      providers:
        - type: forward
rules:
  match_mode: first_match
  rules:
    - name: pick-secondary
      hook: request
      selectors: []
      actions:
        - type: set_lookup_profile
          value: secondary
    - name: retry-once
      hook: response
      selectors: []
      actions:
        - type: retry
"#;
    let cfg = load_yaml(yaml).unwrap();
    let snap = Arc::new(RuntimeSnapshot::from_config(cfg));
    let mut orch = Orchestrator::with_default_stages();
    register_lookup(&mut orch);
    orch.registry.register(Phase::Send, Arc::new(SendStage));

    let txn = run_query(&snap, &orch);
    assert!(
        forward_hits(&txn) >= 2,
        "response retry should re-enter lookup at least once; forward_hits={}",
        forward_hits(&txn)
    );
    assert_eq!(
        tag_string(&txn, "lookup_profile_at_forward").as_deref(),
        Some("secondary")
    );
    assert!(txn.lookup_profile_locked);
}

#[test]
fn unknown_profile_rejected_at_validate() {
    let yaml = two_profile_yaml(
        3,
        r#"
rules:
  match_mode: first_match
  rules:
    - name: bad-profile
      hook: request
      selectors: []
      actions:
        - type: set_lookup_profile
          value: missing
"#,
    );
    let cfg = load_yaml(&yaml).unwrap();
    let report = validate(&cfg);
    assert!(!report.ok);
    assert!(
        report.errors.iter().any(|e| e.contains("missing")),
        "errors: {:?}",
        report.errors
    );
}

#[test]
fn set_lookup_profile_rejected_on_response_hook() {
    let yaml = two_profile_yaml(
        3,
        r#"
rules:
  match_mode: first_match
  rules:
    - name: bad-hook
      hook: response
      selectors: []
      actions:
        - type: set_lookup_profile
          value: secondary
"#,
    );
    let cfg = load_yaml(&yaml).unwrap();
    let report = validate(&cfg);
    assert!(!report.ok);
    assert!(
        report
            .errors
            .iter()
            .any(|e| e.contains("set_lookup_profile") && e.contains("request hook")),
        "errors: {:?}",
        report.errors
    );
}

#[test]
fn rhai_unknown_profile_converges_unknown_profile() {
    let dir = std::env::temp_dir().join(format!("conduit-profile-rhai-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let script_path = dir.join("pick.rhai");
    std::fs::write(
        &script_path,
        r#"txn.set_lookup_profile("not-in-snapshot");"#,
    )
    .unwrap();

    let yaml = format!(
        r#"
schema_version: 1
listeners:
  threads: 1
  listeners:
    - address: "127.0.0.1:15353"
      protocol: udp
forward:
  outstanding_per_backend: 100
  timeout_ms: 2000
orchestrator:
  max_attempts: 3
pools:
  - name: default
    backends:
      - address: "127.0.0.1:5300"
        weight: 100
lookup:
  profiles:
    default:
      providers:
        - type: forward
rhai:
  max_operations: 10000
rules:
  match_mode: first_match
  rules:
    - name: rhai-pick
      hook: request
      selectors: []
      actions:
        - type: rhai
          value: "{}"
"#,
        script_path.display()
    );
    let cfg = load_yaml(&yaml).unwrap();
    assert!(validate(&cfg).ok, "{:?}", validate(&cfg).errors);
    let snap = Arc::new(RuntimeSnapshot::from_config(cfg));

    struct FailForward;
    impl PipelineStage for FailForward {
        fn name(&self) -> &'static str {
            "forward"
        }
        fn handle(&self, _txn: &mut Transaction, _snapshot: &Arc<RuntimeSnapshot>) -> StageOutcome {
            StageOutcome::Continue(Phase::NoAnswer)
        }
    }

    let mut orch = Orchestrator::with_default_stages();
    let lookup = LookupStage::new(
        Arc::new(RouteStage::new()),
        Arc::new(FailForward),
        Arc::new(PassthroughWait),
        None,
    );
    orch.registry.register(Phase::Lookup, Arc::new(lookup));
    orch.registry.register(Phase::Send, Arc::new(SendStage));

    let mut txn = Transaction::new(
        1,
        "127.0.0.1:53".parse::<SocketAddr>().unwrap(),
        ClientProtocol::Udp,
    );
    txn.query_wire = query_wire();

    let outcome = orch.run(&mut txn, &snap, &SystemClock, None);
    assert!(matches!(outcome, RunOutcome::Response(_)));
    assert_eq!(
        txn.convergence_reason,
        Some(ConvergenceReason::UnknownProfile)
    );
    assert_eq!(txn.lookup_profile_name(), "not-in-snapshot");
}

#[test]
fn locked_profile_ignores_late_set() {
    let mut txn = Transaction::new(
        1,
        "127.0.0.1:53".parse::<SocketAddr>().unwrap(),
        ClientProtocol::Udp,
    );
    txn.set_lookup_profile("secondary");
    txn.lock_lookup_profile();
    txn.set_lookup_profile("tertiary");
    assert_eq!(txn.lookup_profile_name(), "secondary");
}
