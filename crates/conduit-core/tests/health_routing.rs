//! Phase B integration: routing follows backend health (eligibility, latency
//! weighting, fail-open) and survives reloads.
//!
//! These drive the wired Route stage over the runtime health side-table owned by
//! the snapshot store — the same `RouteStage::with_health` + `SnapshotStore`
//! path the dataplane runtimes use — without sockets, so the outcomes are
//! deterministic.

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
const B2: &str = "127.0.0.1:5302";

fn addr(s: &str) -> SocketAddr {
    s.parse().unwrap()
}

/// Three equal-weight backends in pool `default` with health enabled
/// (`min_eligible: 1`). `latency_weighting` is configurable per test.
fn config(latency_weighting: bool) -> String {
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
      latency_weighting: {latency_weighting}
    backends:
      - address: "{B0}"
        weight: 100
      - address: "{B1}"
        weight: 100
      - address: "{B2}"
        weight: 100
"#
    )
}

fn store_from(yaml: &str) -> Arc<SnapshotStore> {
    let cfg = load_yaml(yaml).unwrap();
    assert!(conduit_config::validate(&cfg).ok, "config must validate");
    Arc::new(SnapshotStore::new(RuntimeSnapshot::from_config(cfg)))
}

/// Tally selected backends across many transactions through the health-aware
/// Route stage. `None` marks a transaction that got no backend (SERVFAIL).
fn route_distribution(store: &Arc<SnapshotStore>) -> (HashMap<SocketAddr, u32>, u32) {
    let registry = store.health();
    let stage = RouteStage::with_health(registry);
    let snap = store.load();
    let mut counts: HashMap<SocketAddr, u32> = HashMap::new();
    let mut servfail = 0u32;
    for id in 0..3000u64 {
        let mut txn = Transaction::new(id, addr("127.0.0.1:40000"), ClientProtocol::Udp);
        stage.handle(&mut txn, &snap);
        match txn.selected_backend {
            Some(b) => *counts.entry(b).or_insert(0) += 1,
            None => servfail += 1,
        }
    }
    (counts, servfail)
}

#[test]
fn killed_backend_stops_receiving_its_share() {
    let store = store_from(&config(false));
    // Mark one backend down (as a probe fall or fast-trip would).
    store.health().get("default", addr(B1)).unwrap().set_down();

    let (counts, servfail) = route_distribution(&store);
    assert_eq!(servfail, 0, "client answers keep succeeding");
    assert_eq!(counts.get(&addr(B1)), None, "down backend gets no traffic");
    assert!(
        counts[&addr(B0)] > 0 && counts[&addr(B2)] > 0,
        "up backends keep serving"
    );
}

#[test]
fn latency_skew_shifts_traffic() {
    let store = store_from(&config(true));
    // Backend B1 is slow: its damped factor is at the floor; the others stay 1.0.
    store
        .health()
        .get("default", addr(B1))
        .unwrap()
        .damp_weight_factor(0.25, 1.0);

    let (counts, servfail) = route_distribution(&store);
    assert_eq!(servfail, 0);
    let slow = counts[&addr(B1)];
    let fast = counts[&addr(B0)];
    assert!(
        slow < fast,
        "latency-penalized backend receives a smaller share: slow={slow} fast={fast}"
    );
    // Floor 0.25 → effective 25 vs 100; B1 still receives *some* traffic
    // (latency never zeroes a live backend).
    assert!(
        slow > 0,
        "latency only reduces, never removes, a live backend"
    );
}

#[test]
fn all_down_fails_open_no_servfail() {
    let store = store_from(&config(false));
    for b in [B0, B1, B2] {
        store.health().get("default", addr(b)).unwrap().set_down();
    }
    let (counts, servfail) = route_distribution(&store);
    assert_eq!(
        servfail, 0,
        "all-down must fail open, not SERVFAIL for lack of an eligible backend"
    );
    assert_eq!(
        counts.len(),
        3,
        "fail open restores every backend at configured weight"
    );
}

#[test]
fn weight_only_reload_preserves_down_state() {
    let store = store_from(&config(false));
    store.health().get("default", addr(B1)).unwrap().set_down();

    // A weight-only apply: rebuild the snapshot from a config that only changes a
    // backend weight (health identity + probe semantics unchanged). This is the
    // swap path the configurator uses; reconcile must preserve health.
    let mut cfg = load_yaml(&config(false)).unwrap();
    cfg.pools[0].backends[0].weight = Some(50);
    store.swap(RuntimeSnapshot::from_config(cfg));

    assert_eq!(
        store.health().get("default", addr(B1)).unwrap().applied(),
        Health::Down,
        "weight-only reload must not wipe the down backend's health"
    );
    // And routing still excludes it after the reload.
    let (counts, _) = route_distribution(&store);
    assert_eq!(counts.get(&addr(B1)), None);
}

#[test]
fn address_change_reload_resets_health() {
    let store = store_from(&config(false));
    store.health().get("default", addr(B1)).unwrap().set_down();

    // Repoint B1 to a new address: reconcile drops the old entry and seeds the
    // new one from the initial-state policy (optimistic → eligible).
    let mut cfg = load_yaml(&config(false)).unwrap();
    cfg.pools[0].backends[1].address = "127.0.0.1:5399".into();
    store.swap(RuntimeSnapshot::from_config(cfg));

    assert!(
        store.health().get("default", addr(B1)).is_none(),
        "old address dropped from the side-table"
    );
    assert_eq!(
        store
            .health()
            .get("default", addr("127.0.0.1:5399"))
            .unwrap()
            .applied(),
        Health::Up,
        "repointed backend resets to optimistic eligibility"
    );
}
