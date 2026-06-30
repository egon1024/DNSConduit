//! Phase C integration: passive fast-trip opens faster than probe fall; only
//! probe rise closes.

use conduit_config::health::DEFAULT_LATENCY_EWMA_ALPHA;
use conduit_config::load_yaml;
use conduit_core::health::Health;
use conduit_core::pipeline::PipelineStage;
use conduit_core::snapshot::{RuntimeSnapshot, SnapshotStore};
use conduit_core::stages::RouteStage;
use conduit_core::transaction::{ClientProtocol, Transaction};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;

const B0: &str = "127.0.0.1:5300";
const B1: &str = "127.0.0.1:5301";

fn addr(s: &str) -> SocketAddr {
    s.parse().unwrap()
}

/// Slow probe fall (5) vs fast passive trip (2) so passive wins under load.
fn config() -> String {
    format!(
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
  max_txn_duration_ms: 5000
  txn_table_capacity: 1024
pools:
  - name: default
    health:
      enabled: true
      interval_ms: 1000
      rise: 3
      fall: 5
      passive_fall: 2
      min_eligible: 1
    backends:
      - address: "{B0}"
        weight: 100
      - address: "{B1}"
        weight: 100
"#
    )
}

fn store_from(yaml: &str) -> Arc<SnapshotStore> {
    let cfg = load_yaml(yaml).unwrap();
    assert!(conduit_config::validate(&cfg).ok, "config must validate");
    Arc::new(SnapshotStore::new(RuntimeSnapshot::from_config(cfg)))
}

fn route_distribution(store: &Arc<SnapshotStore>) -> HashMap<SocketAddr, u32> {
    let registry = store.health();
    let stage = RouteStage::with_health(registry);
    let snap = store.load();
    let mut counts: HashMap<SocketAddr, u32> = HashMap::new();
    for id in 0..2000u64 {
        let mut txn = Transaction::new(id, addr("127.0.0.1:40000"), ClientProtocol::Udp);
        stage.handle(&mut txn, &snap);
        if let Some(b) = txn.selected_backend {
            *counts.entry(b).or_insert(0) += 1;
        }
    }
    counts
}

#[test]
fn passive_marks_down_faster_than_probe_fall() {
    let store = store_from(&config());
    let registry = store.health();
    let snap = store.load();
    let b1 = addr(B1);

    // Simulate mid-load blackhole: two consecutive forward failures trip passive
    // (passive_fall=2) while probe fall=5 would need five probe failures.
    registry.record_passive_forward_outcome(&snap.health, "default", b1, true);
    assert_eq!(
        registry.get("default", b1).unwrap().applied(),
        Health::Up,
        "one passive failure is not enough"
    );
    registry.record_passive_forward_outcome(&snap.health, "default", b1, true);
    assert_eq!(
        registry.get("default", b1).unwrap().applied(),
        Health::Down,
        "passive opens at passive_fall before probe fall would"
    );

    let counts = route_distribution(&store);
    assert_eq!(counts.get(&b1), None, "down backend drops out of rotation");
    assert!(counts[&addr(B0)] > 0, "peer keeps serving");
}

#[test]
fn passive_down_recovery_requires_probe_rise_not_forward_success() {
    let store = store_from(&config());
    let registry = store.health();
    let snap = store.load();
    let b1 = addr(B1);
    let state = registry.get("default", b1).unwrap();
    const ALPHA: f64 = DEFAULT_LATENCY_EWMA_ALPHA;

    registry.record_passive_forward_outcome(&snap.health, "default", b1, true);
    registry.record_passive_forward_outcome(&snap.health, "default", b1, true);
    assert_eq!(state.applied(), Health::Down);

    // Blackhole removed: live traffic succeeds again — must not close.
    for _ in 0..5 {
        registry.record_passive_forward_outcome(&snap.health, "default", b1, false);
    }
    assert_eq!(
        state.applied(),
        Health::Down,
        "forward success alone must not mark up"
    );

    // Probe rise closes (rise=3).
    assert_eq!(state.record_success(3, ALPHA, 1.0), Health::Down);
    assert_eq!(state.record_success(3, ALPHA, 1.0), Health::Down);
    assert_eq!(state.record_success(3, ALPHA, 1.0), Health::Up);

    let counts = route_distribution(&store);
    assert!(
        counts.contains_key(&b1),
        "recovered backend re-enters rotation"
    );
}
