mod support;

use conduit_proto::control::conduit_control_client::ConduitControlClient;
use conduit_proto::control::HealthRequest;
use std::net::SocketAddr;

#[tokio::test]
async fn health_returns_serving() {
    let (snapshots, effective, configurator, tracing, base_dir) = support::minimal_control_setup();

    let addr: SocketAddr = "127.0.0.1:0".parse().expect("parse");
    let local_addr =
        conduit_api::serve_on_listener(addr, snapshots, effective, configurator, tracing, base_dir)
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
