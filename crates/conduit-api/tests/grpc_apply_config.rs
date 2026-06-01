mod support;

use conduit_config::load_yaml;
use conduit_proto::config::Config as RuntimeConfig;
use conduit_proto::control::conduit_control_client::ConduitControlClient;
use conduit_proto::control::Config as ControlConfig;
use conduit_proto::control::{ApplyConfigRequest, GetConfigRequest, ReloadFromFileRequest};
use prost::Message;
use std::net::SocketAddr;

fn runtime_to_control(cfg: RuntimeConfig) -> ControlConfig {
    let bytes = cfg.encode_to_vec();
    ControlConfig::decode(bytes.as_slice()).expect("compatible")
}

#[tokio::test]
async fn apply_config_changes_pool_weight_and_generation() {
    let (snapshots, effective, configurator, tracing) = support::minimal_control_setup();
    let gen0 = snapshots.generation();

    let addr: SocketAddr = "127.0.0.1:0".parse().expect("parse");
    let local_addr =
        conduit_api::serve_on_listener(addr, snapshots.clone(), effective, configurator, tracing)
            .await
            .expect("start server");

    let mut client = ConduitControlClient::connect(format!("http://{local_addr}"))
        .await
        .expect("connect");

    let yaml = include_str!("../../../tests/fixtures/config/minimal.yaml");
    let file_cfg = load_yaml(yaml).expect("parse");
    let mut overlay = file_cfg.clone();
    overlay.pools[0].backends[0].weight = Some(7);

    let apply = client
        .apply_config(ApplyConfigRequest {
            overlay: Some(runtime_to_control(overlay)),
        })
        .await
        .expect("apply")
        .into_inner();
    assert!(apply.ok, "{:?}", apply.errors);
    assert!(snapshots.generation() > gen0);

    let got = client
        .get_config(GetConfigRequest {})
        .await
        .expect("get config")
        .into_inner()
        .effective
        .expect("effective");
    assert_eq!(got.pools[0].backends[0].weight, Some(7));
}

#[tokio::test]
async fn apply_config_invalid_overlay_rejected() {
    let (snapshots, effective, configurator, tracing) = support::minimal_control_setup();
    let gen0 = snapshots.generation();

    let addr: SocketAddr = "127.0.0.1:0".parse().expect("parse");
    let local_addr =
        conduit_api::serve_on_listener(addr, snapshots.clone(), effective, configurator, tracing)
            .await
            .expect("start server");

    let mut client = ConduitControlClient::connect(format!("http://{local_addr}"))
        .await
        .expect("connect");

    let yaml = include_str!("../../../tests/fixtures/config/minimal.yaml");
    let mut overlay = load_yaml(yaml).expect("parse");
    overlay.listeners.as_mut().unwrap().threads = 0;

    let apply = client
        .apply_config(ApplyConfigRequest {
            overlay: Some(runtime_to_control(overlay)),
        })
        .await
        .expect("apply rpc")
        .into_inner();
    assert!(!apply.ok);
    assert_eq!(snapshots.generation(), gen0);
    assert_eq!(
        snapshots.load().config.pools[0].backends[0].weight,
        Some(100)
    );
}

#[tokio::test]
async fn reload_from_file_clears_api_overlay() {
    let (snapshots, effective, configurator, tracing) = support::minimal_control_setup();

    let addr: SocketAddr = "127.0.0.1:0".parse().expect("parse");
    let local_addr =
        conduit_api::serve_on_listener(addr, snapshots.clone(), effective, configurator, tracing)
            .await
            .expect("start server");

    let mut client = ConduitControlClient::connect(format!("http://{local_addr}"))
        .await
        .expect("connect");

    let yaml = include_str!("../../../tests/fixtures/config/minimal.yaml");
    let mut overlay = load_yaml(yaml).expect("parse");
    overlay.pools[0].backends[0].weight = Some(11);

    client
        .apply_config(ApplyConfigRequest {
            overlay: Some(runtime_to_control(overlay)),
        })
        .await
        .expect("apply");
    assert_eq!(
        snapshots.load().config.pools[0].backends[0].weight,
        Some(11)
    );

    let reload = client
        .reload_from_file(ReloadFromFileRequest {})
        .await
        .expect("reload")
        .into_inner();
    assert!(reload.ok, "{:?}", reload.errors);
    assert_eq!(
        snapshots.load().config.pools[0].backends[0].weight,
        Some(100)
    );
}
