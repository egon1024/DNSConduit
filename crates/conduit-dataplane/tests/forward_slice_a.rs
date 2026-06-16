//! Phase 1b slice A: forward sources_v4 and compiled snapshot.

use conduit_config::{load_yaml, validate};
use conduit_core::snapshot::RuntimeSnapshot;

#[test]
fn forward_sources_v4_fixture_compiles() {
    let yaml = include_str!("../../../tests/fixtures/config/forward-sources-v4.yaml");
    let cfg = load_yaml(yaml).unwrap();
    assert!(validate(&cfg).ok);
    let snap = RuntimeSnapshot::from_config(cfg);
    assert_eq!(snap.forward.sources_v4.len(), 1);
}

#[test]
fn forward_sources_v6_fixture_compiles() {
    let yaml = include_str!("../../../tests/fixtures/config/forward-sources-v6.yaml");
    let cfg = load_yaml(yaml).unwrap();
    assert!(validate(&cfg).ok);
    let snap = RuntimeSnapshot::from_config(cfg);
    assert_eq!(snap.forward.sources_v6.len(), 1);
}

#[test]
fn minimal_config_default_forward_unchanged() {
    let yaml = include_str!("../../../tests/fixtures/config/dataplane-minimal.yaml");
    let cfg = load_yaml(yaml).unwrap();
    let snap = RuntimeSnapshot::from_config(cfg);
    assert!(snap.forward.sources_v4.is_empty());
}
