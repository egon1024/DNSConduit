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
async fn validate_config_accepts_collect_off_with_script_warning_path() {
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

    let collect_off = load_yaml(include_str!(
        "../../../tests/fixtures/config/metrics-consumer-collect-removed.yaml"
    ))
    .expect("parse collect-off");
    let resp: ValidateConfigResponse = client
        .validate_config(ValidateConfigRequest {
            config: Some(runtime_to_control(collect_off)),
        })
        .await
        .expect("validate rpc")
        .into_inner();

    assert!(
        resp.ok,
        "collect-off write sites must validate ok; errors: {}",
        resp.errors.join("\n")
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
