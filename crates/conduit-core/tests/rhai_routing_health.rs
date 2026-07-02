//! Rhai `runtime.routing` reads track health side-table state (phase 1c + runtime host API).

use conduit_config::load_yaml;
use conduit_core::build_routing_runtime_snapshot;
use conduit_core::health::HealthRegistry;
use conduit_core::pipeline::PipelineStage;
use conduit_core::snapshot::RuntimeSnapshot;
use conduit_core::stages::request_rules::RequestRulesStage;
use conduit_core::transaction::{ClientProtocol, Transaction};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

fn fixtures_config_base() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/config")
}

fn example_query() -> Vec<u8> {
    [
        0x12, 0x34, 0x01, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x03, 0x77, 0x77,
        0x77, 0x07, 0x65, 0x78, 0x61, 0x6d, 0x70, 0x6c, 0x65, 0x03, 0x63, 0x6f, 0x6d, 0x00, 0x00,
        0x01, 0x00, 0x01,
    ]
    .to_vec()
}

fn addr(s: &str) -> SocketAddr {
    s.parse().unwrap()
}

#[test]
fn request_script_switches_pool_when_eligible_count_drops() {
    let yaml = include_str!("../../../tests/fixtures/config/with-rhai-routing-pool.yaml");
    let cfg = load_yaml(yaml).unwrap();
    let snap = Arc::new(RuntimeSnapshot::from_config_with_base(
        cfg.clone(),
        Some(&fixtures_config_base()),
    ));
    let registry = Arc::new(HealthRegistry::from_compiled(&snap.health));

    registry
        .get("primary", addr("127.0.0.1:15300"))
        .unwrap()
        .set_down();

    let stage = RequestRulesStage {
        metrics: None,
        health: Some(registry),
        outstanding: None,
    };

    let mut txn = Transaction::new(1, addr("127.0.0.1:15353"), ClientProtocol::Udp)
        .with_query_wire(example_query());
    let _ = stage.handle(&mut txn, &snap);
    assert_eq!(txn.selected_pool.as_deref(), Some("secondary"));
}

#[test]
fn routing_snapshot_eligible_count_matches_induced_down_backend() {
    let yaml = include_str!("../../../tests/fixtures/config/with-health.yaml");
    let cfg = load_yaml(yaml).unwrap();
    let snap = RuntimeSnapshot::from_config(cfg.clone());
    let registry = HealthRegistry::from_compiled(&snap.health);
    registry
        .get("default", addr("127.0.0.1:5300"))
        .unwrap()
        .set_down();

    let routing = build_routing_runtime_snapshot(
        &cfg,
        &snap.health,
        &registry,
        &std::collections::HashMap::new(),
        snap.generation,
    );
    let pool = routing.pool("default");
    assert_eq!(pool.configured_count, 2);
    assert_eq!(pool.eligible_count, 1);

    let backend = routing.backend("default", "127.0.0.1:5300");
    assert!(backend.configured);
    assert!(!backend.eligible);
    assert_eq!(backend.applied, "down");
}
