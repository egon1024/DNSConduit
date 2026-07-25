mod support;

use conduit_config::load_yaml;
use conduit_proto::control::conduit_control_client::ConduitControlClient;
use conduit_proto::control::{ValidateConfigRequest, ValidateConfigResponse};
use prost::Message;
use std::net::SocketAddr;

fn runtime_to_control(cfg: conduit_proto::config::Config) -> conduit_proto::control::Config {
    let bytes = cfg.encode_to_vec();
    conduit_proto::control::Config::decode(bytes.as_slice()).expect("compatible")
}

#[tokio::test]
async fn validate_config_rejects_collect_removal_with_script_path() {
    let yaml = include_str!("../../../tests/fixtures/config/metrics-consumer-blat-base.yaml");
    let file_cfg = load_yaml(yaml).expect("parse");
    let (snapshots, effective, configurator, tracing, base_dir) = support::control_setup(
        file_cfg,
        support::workspace_fixture("tests/fixtures/config/metrics-consumer-blat-base.yaml"),
        Some(support::workspace_fixture("tests/fixtures/config")),
    );

    let addr: SocketAddr = "127.0.0.1:0".parse().expect("parse");
    let local_addr =
        conduit_api::serve_on_listener(addr, snapshots, effective, configurator, tracing, base_dir)
            .await
            .expect("start server");

    let mut client = ConduitControlClient::connect(format!("http://{local_addr}"))
        .await
        .expect("connect");

    let bad = load_yaml(include_str!(
        "../../../tests/fixtures/config/metrics-consumer-collect-removed.yaml"
    ))
    .expect("parse bad");
    let resp: ValidateConfigResponse = client
        .validate_config(ValidateConfigRequest {
            config: Some(runtime_to_control(bad)),
        })
        .await
        .expect("validate rpc")
        .into_inner();

    assert!(!resp.ok, "expected validation failure");
    let joined = resp.errors.join("\n");
    assert!(
        joined.contains("cannot stop collecting metric \"blat\""),
        "errors: {joined}"
    );
    assert!(
        joined.contains("consumer-blat.rhai"),
        "errors should list script path: {joined}"
    );
}

#[tokio::test]
async fn validate_config_accepts_collecting_referenced_metric() {
    let yaml = include_str!("../../../tests/fixtures/config/metrics-consumer-blat-base.yaml");
    let file_cfg = load_yaml(yaml).expect("parse");
    let (snapshots, effective, configurator, tracing, base_dir) = support::control_setup(
        file_cfg.clone(),
        support::workspace_fixture("tests/fixtures/config/metrics-consumer-blat-base.yaml"),
        Some(support::workspace_fixture("tests/fixtures/config")),
    );

    let addr: SocketAddr = "127.0.0.1:0".parse().expect("parse");
    let local_addr =
        conduit_api::serve_on_listener(addr, snapshots, effective, configurator, tracing, base_dir)
            .await
            .expect("start server");

    let mut client = ConduitControlClient::connect(format!("http://{local_addr}"))
        .await
        .expect("connect");

    let resp = client
        .validate_config(ValidateConfigRequest {
            config: Some(runtime_to_control(file_cfg)),
        })
        .await
        .expect("validate rpc")
        .into_inner();

    assert!(resp.ok, "errors: {:?}", resp.errors);
}
