use conduit_config::{load_yaml, EffectiveConfig};
use conduit_core::{RuntimeSnapshot, SnapshotStore};
use conduit_metrics::TracingHub;
use conduit_proto::control::conduit_control_client::ConduitControlClient;
use conduit_proto::control::HealthRequest;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};

#[tokio::test]
async fn health_returns_serving() {
    let yaml = include_str!("../../../tests/fixtures/config/minimal.yaml");
    let file_cfg = load_yaml(yaml).expect("parse");
    let snapshots = Arc::new(SnapshotStore::new(RuntimeSnapshot::from_config(
        file_cfg.clone(),
    )));
    let tracing = Arc::new(TracingHub::from_config(&file_cfg));
    let effective = Arc::new(Mutex::new(EffectiveConfig::new(file_cfg)));

    let addr: SocketAddr = "127.0.0.1:0".parse().expect("parse");
    let local_addr = conduit_api::serve_on_listener(addr, snapshots, effective, tracing)
        .await
        .expect("start server");

    let endpoint = format!("http://{local_addr}");
    let mut client = ConduitControlClient::connect(endpoint)
        .await
        .expect("connect");

    let response = client
        .health(HealthRequest {})
        .await
        .expect("health rpc")
        .into_inner();

    assert_eq!(response.status, "serving");
}
