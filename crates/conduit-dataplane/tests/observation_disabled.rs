use conduit_config::{load_yaml, validate};
use conduit_core::{RuntimeSnapshot, SnapshotStore};
use conduit_dataplane::supervisor;
use conduit_metrics::{MetricsHub, TracingHub};
use std::sync::Arc;

#[test]
fn no_sinks_observation_has_zero_consumer_threads() {
    let yaml = include_str!("../../../tests/fixtures/config/no-sinks.yaml");
    let cfg = load_yaml(yaml).unwrap();
    assert!(validate(&cfg).ok);
    let store = Arc::new(SnapshotStore::new(RuntimeSnapshot::from_config(
        cfg.clone(),
    )));
    let metrics = Arc::new(MetricsHub::from_config(&cfg));
    let tracing = Arc::new(TracingHub::from_config(&cfg));
    let handle = supervisor::start(store, metrics, tracing).unwrap();
    assert!(!handle.observation.enabled());
    assert_eq!(handle.observation.consumer_count(), 0);
}
