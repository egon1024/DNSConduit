mod support;

use conduit_config::load_yaml;
use conduit_proto::config::{Backend as RuntimeBackend, Config as RuntimeConfig};
use conduit_proto::control::conduit_control_client::ConduitControlClient;
use conduit_proto::control::conduit_pools_client::ConduitPoolsClient;
use conduit_proto::control::Config as ControlConfig;
use conduit_proto::control::{
    ApplyConfigRequest, ExportConfigRequest, ListPoolsRequest, OverlayApplyMode,
    RemoveBackendRequest, SetBackendWeightRequest,
};
use prost::Message;
use std::net::SocketAddr;

fn runtime_to_control(cfg: RuntimeConfig) -> ControlConfig {
    let bytes = cfg.encode_to_vec();
    ControlConfig::decode(bytes.as_slice()).expect("compatible")
}

fn two_backend_setup() -> support::ControlSetup {
    let yaml = include_str!("../../../tests/fixtures/config/minimal.yaml");
    let mut file_cfg = load_yaml(yaml).expect("parse");
    file_cfg.pools[0].name = "edge".into();
    file_cfg.pools[0].backends = vec![
        RuntimeBackend {
            name: Some("primary".into()),
            address: "127.0.0.1:5300".into(),
            weight: Some(100),
            ..Default::default()
        },
        RuntimeBackend {
            name: Some("secondary".into()),
            address: "127.0.0.1:5301".into(),
            weight: Some(100),
            ..Default::default()
        },
    ];
    support::control_setup(
        file_cfg,
        support::workspace_fixture("tests/fixtures/config/minimal.yaml"),
        Some(support::workspace_fixture("tests/fixtures/config")),
    )
}

fn weight_overlay(pool: &str, backend: &str, weight: u32) -> ControlConfig {
    let mut cfg = RuntimeConfig {
        schema_version: 1,
        ..Default::default()
    };
    cfg.pools = vec![conduit_proto::config::Pool {
        name: pool.into(),
        backends: vec![RuntimeBackend {
            name: Some(backend.into()),
            weight: Some(weight),
            ..Default::default()
        }],
        ..Default::default()
    }];
    runtime_to_control(cfg)
}

#[tokio::test]
async fn set_backend_weight_and_export_reflects() {
    let (snapshots, effective, configurator, tracing, base_dir) = two_backend_setup();
    let gen0 = snapshots.generation();

    let addr: SocketAddr = "127.0.0.1:0".parse().expect("parse");
    let local_addr = conduit_api::serve_on_listener(
        addr,
        snapshots.clone(),
        effective,
        configurator,
        tracing,
        base_dir,
    )
    .await
    .expect("start server");

    let mut pools = ConduitPoolsClient::connect(format!("http://{local_addr}"))
        .await
        .expect("connect pools");
    let mut control = ConduitControlClient::connect(format!("http://{local_addr}"))
        .await
        .expect("connect control");

    let listed = pools
        .list_pools(ListPoolsRequest {})
        .await
        .expect("list")
        .into_inner();
    assert_eq!(listed.pools.len(), 1);
    assert_eq!(listed.pools[0].name, "edge");
    assert_eq!(listed.pools[0].backend_count, 2);

    let set = pools
        .set_backend_weight(SetBackendWeightRequest {
            pool: "edge".into(),
            backend: "primary".into(),
            weight: 42,
        })
        .await
        .expect("set-weight")
        .into_inner();
    assert!(set.ok, "{:?}", set.errors);
    assert!(set.generation > gen0);
    assert_eq!(set.generation, snapshots.generation());

    let export = control
        .export_config(ExportConfigRequest {
            format: "yaml".into(),
        })
        .await
        .expect("export")
        .into_inner()
        .body;
    assert!(
        export.contains("weight: 42") || export.contains("weight:42"),
        "export should show updated weight:\n{export}"
    );
    assert!(export.contains("primary"), "{export}");
    assert!(export.contains("secondary"), "{export}");
}

#[tokio::test]
async fn remove_backend_and_export_clean() {
    let (snapshots, effective, configurator, tracing, base_dir) = two_backend_setup();

    let addr: SocketAddr = "127.0.0.1:0".parse().expect("parse");
    let local_addr = conduit_api::serve_on_listener(
        addr,
        snapshots.clone(),
        effective,
        configurator,
        tracing,
        base_dir,
    )
    .await
    .expect("start server");

    let mut pools = ConduitPoolsClient::connect(format!("http://{local_addr}"))
        .await
        .expect("connect pools");
    let mut control = ConduitControlClient::connect(format!("http://{local_addr}"))
        .await
        .expect("connect control");

    let rem = pools
        .remove_backend(RemoveBackendRequest {
            pool: "edge".into(),
            backend: "secondary".into(),
        })
        .await
        .expect("remove")
        .into_inner();
    assert!(rem.ok, "{:?}", rem.errors);

    assert_eq!(snapshots.load().config.pools[0].backends.len(), 1);
    assert_eq!(
        snapshots.load().config.pools[0].backends[0].name.as_deref(),
        Some("primary")
    );

    let export = control
        .export_config(ExportConfigRequest {
            format: "yaml".into(),
        })
        .await
        .expect("export")
        .into_inner()
        .body;
    assert!(export.contains("primary"), "{export}");
    assert!(
        !export.contains("secondary"),
        "export must not list removed backend:\n{export}"
    );
    assert!(
        !export.contains("remove:"),
        "export must not emit remove markers:\n{export}"
    );
}

#[tokio::test]
async fn interleaved_document_apply_and_primitive() {
    let (snapshots, effective, configurator, tracing, base_dir) = two_backend_setup();

    let addr: SocketAddr = "127.0.0.1:0".parse().expect("parse");
    let local_addr = conduit_api::serve_on_listener(
        addr,
        snapshots.clone(),
        effective,
        configurator,
        tracing,
        base_dir,
    )
    .await
    .expect("start server");

    let mut pools = ConduitPoolsClient::connect(format!("http://{local_addr}"))
        .await
        .expect("connect pools");
    let mut control = ConduitControlClient::connect(format!("http://{local_addr}"))
        .await
        .expect("connect control");

    let apply = control
        .apply_config(ApplyConfigRequest {
            overlay: Some(weight_overlay("edge", "primary", 11)),
            mode: OverlayApplyMode::Merge.into(),
        })
        .await
        .expect("apply")
        .into_inner();
    assert!(apply.ok, "{:?}", apply.errors);

    let set = pools
        .set_backend_weight(SetBackendWeightRequest {
            pool: "edge".into(),
            backend: "secondary".into(),
            weight: 22,
        })
        .await
        .expect("set-weight")
        .into_inner();
    assert!(set.ok, "{:?}", set.errors);

    let snap = snapshots.load();
    let backends = &snap.config.pools[0].backends;
    let primary = backends
        .iter()
        .find(|b| b.name.as_deref() == Some("primary"))
        .expect("primary");
    let secondary = backends
        .iter()
        .find(|b| b.name.as_deref() == Some("secondary"))
        .expect("secondary");
    assert_eq!(primary.weight, Some(11));
    assert_eq!(secondary.weight, Some(22));
}
