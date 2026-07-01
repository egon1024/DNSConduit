//! Phase D integration: operator health controls affect routing while observed
//! and applied can diverge under freeze.

use conduit_config::health::DEFAULT_LATENCY_EWMA_ALPHA;
use conduit_config::load_yaml;
use conduit_core::health::{EffectiveScope, Health, HealthControlAction, HealthControlScope};
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
    assert!(conduit_config::validate(&cfg).ok);
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
fn drain_removes_traffic_while_observed_stays_up_until_resume() {
    let store = store_from(&config());
    let registry = store.health();
    let snap = store.load();
    let b1 = addr(B1);
    let state = registry.get("default", b1).unwrap();
    const ALPHA: f64 = DEFAULT_LATENCY_EWMA_ALPHA;

    registry
        .set_health_control(
            &snap.health,
            HealthControlScope::Backend {
                pool: "default".into(),
                address: b1,
            },
            HealthControlAction::SetDown,
        )
        .unwrap();
    // Probes still see the backend up.
    state.record_success(1, ALPHA, 1.0);
    assert_eq!(state.observed(), Health::Up);
    assert_eq!(state.applied(), Health::Down);

    let counts = route_distribution(&store);
    assert_eq!(counts.get(&b1), None, "drained backend gets no traffic");
    assert!(counts[&addr(B0)] > 0);

    registry
        .set_health_control(
            &snap.health,
            HealthControlScope::Backend {
                pool: "default".into(),
                address: b1,
            },
            HealthControlAction::ResumeAutomatic,
        )
        .unwrap();
    assert_eq!(state.applied(), Health::Up, "resume snaps to observed");

    let counts = route_distribution(&store);
    assert!(
        counts.contains_key(&b1),
        "resumed backend returns to rotation immediately"
    );
}

#[test]
fn global_freeze_with_backend_automatic_carve_out() {
    let store = store_from(&config());
    let registry = store.health();
    let snap = store.load();
    let b0 = addr(B0);
    let b1 = addr(B1);
    const ALPHA: f64 = DEFAULT_LATENCY_EWMA_ALPHA;

    registry
        .set_health_control(
            &snap.health,
            HealthControlScope::Global,
            HealthControlAction::Freeze,
        )
        .unwrap();

    // Carve-out: one backend resumes automatic and follows probes.
    registry
        .set_health_control(
            &snap.health,
            HealthControlScope::Backend {
                pool: "default".into(),
                address: b1,
            },
            HealthControlAction::ResumeAutomatic,
        )
        .unwrap();
    registry
        .get("default", b1)
        .unwrap()
        .record_success(1, ALPHA, 1.0);

    registry.get("default", b0).unwrap().record_failure(2);
    registry.get("default", b0).unwrap().record_failure(2);
    registry.get("default", b1).unwrap().record_failure(2);
    registry.get("default", b1).unwrap().record_failure(2);

    assert_eq!(
        registry.get("default", b0).unwrap().applied(),
        Health::Up,
        "global freeze holds applied on frozen backend"
    );
    assert_eq!(
        registry.get("default", b1).unwrap().applied(),
        Health::Down,
        "carve-out backend follows probe truth"
    );
    assert_eq!(
        registry.resolve_scope("default", b1),
        EffectiveScope::Automatic
    );
    assert_eq!(
        registry.resolve_scope("default", b0),
        EffectiveScope::Frozen
    );

    // Global resume must not un-drain a backend that was individually set down.
    registry
        .set_health_control(
            &snap.health,
            HealthControlScope::Backend {
                pool: "default".into(),
                address: b0,
            },
            HealthControlAction::SetDown,
        )
        .unwrap();
    registry
        .set_health_control(
            &snap.health,
            HealthControlScope::Global,
            HealthControlAction::ResumeAutomatic,
        )
        .unwrap();
    registry
        .get("default", b0)
        .unwrap()
        .record_success(1, ALPHA, 1.0);
    assert_eq!(
        registry.get("default", b0).unwrap().applied(),
        Health::Down,
        "individually drained backend stays down after global resume"
    );
}
