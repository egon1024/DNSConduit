mod support;

use conduit_config::load_yaml;
use std::net::SocketAddr;
use tokio_stream::StreamExt;
use tonic::Request;
use tonic_reflection::pb::v1alpha::{
    server_reflection_client::ServerReflectionClient, server_reflection_request::MessageRequest,
    server_reflection_response::MessageResponse, ServerReflectionRequest,
};

async fn list_services(addr: SocketAddr) -> Result<Vec<String>, tonic::Status> {
    let endpoint = format!("http://{addr}");
    let conn = tonic::transport::Endpoint::new(endpoint)
        .expect("endpoint")
        .connect()
        .await
        .expect("connect");
    let mut client = ServerReflectionClient::new(conn);
    let request = Request::new(tokio_stream::once(ServerReflectionRequest {
        host: String::new(),
        message_request: Some(MessageRequest::ListServices(String::new())),
    }));
    let mut inbound = client.server_reflection_info(request).await?.into_inner();
    let response = inbound.next().await.expect("stream item")?;
    let message = response.message_response.expect("message response");
    match message {
        MessageResponse::ListServicesResponse(services) => {
            Ok(services.service.into_iter().map(|s| s.name).collect())
        }
        _ => Ok(Vec::new()),
    }
}

#[tokio::test]
async fn reflection_enabled_lists_conduit_service() {
    let yaml = include_str!("../../../tests/fixtures/config/with-metrics-tracing-prometheus.yaml");
    let file_cfg = load_yaml(yaml).expect("parse");
    assert!(
        file_cfg
            .control
            .as_ref()
            .map(|c| c.reflection_enabled)
            .unwrap_or(false),
        "fixture should enable reflection"
    );
    let (snapshots, effective, configurator, tracing, base_dir) = support::control_setup(
        file_cfg,
        support::workspace_fixture("tests/fixtures/config/with-metrics-tracing-prometheus.yaml"),
        Some(support::workspace_fixture("tests/fixtures/config")),
    );

    let addr: SocketAddr = "127.0.0.1:0".parse().expect("parse");
    let local_addr =
        conduit_api::serve_on_listener(addr, snapshots, effective, configurator, tracing, base_dir)
            .await
            .expect("start server");

    let services = list_services(local_addr).await.expect("reflection list");
    assert!(
        services.iter().any(|s| s == "conduit.v1.ConduitControl"),
        "services={services:?}"
    );
    assert!(
        services.iter().any(|s| s == "conduit.v1.ConduitPools"),
        "services={services:?}"
    );
}

#[tokio::test]
async fn reflection_disabled_rejects_reflection_requests() {
    let yaml = include_str!("../../../tests/fixtures/config/minimal.yaml");
    let file_cfg = load_yaml(yaml).expect("parse");
    assert!(
        !file_cfg
            .control
            .as_ref()
            .map(|c| c.reflection_enabled)
            .unwrap_or(false),
        "minimal fixture should not enable reflection"
    );
    let (snapshots, effective, configurator, tracing, base_dir) = support::minimal_control_setup();

    let addr: SocketAddr = "127.0.0.1:0".parse().expect("parse");
    let local_addr =
        conduit_api::serve_on_listener(addr, snapshots, effective, configurator, tracing, base_dir)
            .await
            .expect("start server");

    let err = list_services(local_addr)
        .await
        .expect_err("reflection should be unavailable");
    assert_eq!(err.code(), tonic::Code::Unimplemented);
}
