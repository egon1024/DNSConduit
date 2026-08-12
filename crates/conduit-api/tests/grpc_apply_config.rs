mod support;

use conduit_config::load_yaml;
use conduit_proto::config::Config as RuntimeConfig;
use conduit_proto::control::conduit_control_client::ConduitControlClient;
use conduit_proto::control::Config as ControlConfig;
use conduit_proto::control::{
    ApplyConfigRequest, GetConfigRequest, OverlayApplyMode, ReloadFromFileRequest,
};
use prost::Message;
use std::net::SocketAddr;

fn runtime_to_control(cfg: RuntimeConfig) -> ControlConfig {
    let bytes = cfg.encode_to_vec();
    ControlConfig::decode(bytes.as_slice()).expect("compatible")
}

fn pool_weight_overlay(file_cfg: &RuntimeConfig, weight: u32) -> ControlConfig {
    let mut cfg = RuntimeConfig {
        schema_version: 1,
        ..Default::default()
    };
    let mut pool = file_cfg.pools[0].clone();
    pool.backends[0].weight = Some(weight);
    cfg.pools = vec![pool];
    runtime_to_control(cfg)
}

#[tokio::test]
async fn apply_config_changes_pool_weight_and_generation() {
    let (snapshots, effective, configurator, tracing, base_dir) = support::minimal_control_setup();
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

    let mut client = ConduitControlClient::connect(format!("http://{local_addr}"))
        .await
        .expect("connect");

    let yaml = include_str!("../../../tests/fixtures/config/minimal.yaml");
    let file_cfg = load_yaml(yaml).expect("parse");

    let apply = client
        .apply_config(ApplyConfigRequest {
            overlay: Some(pool_weight_overlay(&file_cfg, 7)),
            mode: OverlayApplyMode::Merge.into(),
        })
        .await
        .expect("apply")
        .into_inner();
    assert!(apply.ok, "{:?}", apply.errors);
    assert!(
        apply.generation > gen0,
        "response generation {} should exceed prior {}",
        apply.generation,
        gen0
    );
    assert!(snapshots.generation() > gen0);
    assert_eq!(apply.generation, snapshots.generation());
    // Fully hot weight apply may leave notes empty (proto3 additive).
    assert!(apply.notes.is_empty() || apply.notes.iter().all(|n| !n.kind.is_empty()));

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
    let (snapshots, effective, configurator, tracing, base_dir) = support::minimal_control_setup();
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

    let mut client = ConduitControlClient::connect(format!("http://{local_addr}"))
        .await
        .expect("connect");

    let yaml = include_str!("../../../tests/fixtures/config/minimal.yaml");
    let file_cfg = load_yaml(yaml).expect("parse");
    let mut listeners = file_cfg.listeners.clone().expect("listeners");
    listeners.threads = 0;
    let overlay = runtime_to_control(RuntimeConfig {
        schema_version: 1,
        listeners: Some(listeners),
        ..Default::default()
    });

    let apply = client
        .apply_config(ApplyConfigRequest {
            overlay: Some(overlay),
            mode: OverlayApplyMode::Merge.into(),
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
    let (snapshots, effective, configurator, tracing, base_dir) = support::minimal_control_setup();

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

    let mut client = ConduitControlClient::connect(format!("http://{local_addr}"))
        .await
        .expect("connect");

    let yaml = include_str!("../../../tests/fixtures/config/minimal.yaml");
    let file_cfg = load_yaml(yaml).expect("parse");

    client
        .apply_config(ApplyConfigRequest {
            overlay: Some(pool_weight_overlay(&file_cfg, 11)),
            mode: OverlayApplyMode::Merge.into(),
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
    assert!(reload.generation > 0);
    assert_eq!(reload.generation, snapshots.generation());
    assert_eq!(
        snapshots.load().config.pools[0].backends[0].weight,
        Some(100)
    );
}

#[tokio::test]
async fn apply_config_merge_accumulates_overlay() {
    let (snapshots, effective, configurator, tracing, base_dir) = support::minimal_control_setup();

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

    let mut client = ConduitControlClient::connect(format!("http://{local_addr}"))
        .await
        .expect("connect");

    let yaml = include_str!("../../../tests/fixtures/config/minimal.yaml");
    let file_cfg = load_yaml(yaml).expect("parse");

    client
        .apply_config(ApplyConfigRequest {
            overlay: Some(pool_weight_overlay(&file_cfg, 50)),
            mode: OverlayApplyMode::Merge.into(),
        })
        .await
        .expect("apply weight");

    let listeners = runtime_to_control(RuntimeConfig {
        schema_version: 1,
        listeners: Some(conduit_proto::config::ListenersConfig {
            threads: 4,
            reuse_port: true,
            rcvbuf: 0,
            sndbuf: 0,
            listeners: vec![],
        }),
        ..Default::default()
    });

    client
        .apply_config(ApplyConfigRequest {
            overlay: Some(listeners),
            mode: OverlayApplyMode::Merge.into(),
        })
        .await
        .expect("apply listeners");

    let got = client
        .get_config(GetConfigRequest {})
        .await
        .expect("get config")
        .into_inner()
        .effective
        .expect("effective");
    assert_eq!(got.pools[0].backends[0].weight, Some(50));
    assert_eq!(got.listeners.as_ref().unwrap().threads, 4);
}

#[tokio::test]
async fn apply_config_clear_without_reload() {
    let (snapshots, effective, configurator, tracing, base_dir) = support::minimal_control_setup();

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

    let mut client = ConduitControlClient::connect(format!("http://{local_addr}"))
        .await
        .expect("connect");

    let yaml = include_str!("../../../tests/fixtures/config/minimal.yaml");
    let file_cfg = load_yaml(yaml).expect("parse");

    client
        .apply_config(ApplyConfigRequest {
            overlay: Some(pool_weight_overlay(&file_cfg, 50)),
            mode: OverlayApplyMode::Merge.into(),
        })
        .await
        .expect("apply");

    let clear = client
        .apply_config(ApplyConfigRequest {
            overlay: None,
            mode: OverlayApplyMode::Clear.into(),
        })
        .await
        .expect("clear")
        .into_inner();
    assert!(clear.ok, "{:?}", clear.errors);
    assert_eq!(
        snapshots.load().config.pools[0].backends[0].weight,
        Some(100)
    );
}

#[tokio::test]
async fn apply_config_replace_empty_clears_overlay() {
    let (snapshots, effective, configurator, tracing, base_dir) = support::minimal_control_setup();

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

    let mut client = ConduitControlClient::connect(format!("http://{local_addr}"))
        .await
        .expect("connect");

    let yaml = include_str!("../../../tests/fixtures/config/minimal.yaml");
    let file_cfg = load_yaml(yaml).expect("parse");

    client
        .apply_config(ApplyConfigRequest {
            overlay: Some(pool_weight_overlay(&file_cfg, 50)),
            mode: OverlayApplyMode::Merge.into(),
        })
        .await
        .expect("apply");

    let empty = runtime_to_control(RuntimeConfig {
        schema_version: 1,
        ..Default::default()
    });

    client
        .apply_config(ApplyConfigRequest {
            overlay: Some(empty),
            mode: OverlayApplyMode::Replace.into(),
        })
        .await
        .expect("replace empty");

    assert_eq!(
        snapshots.load().config.pools[0].backends[0].weight,
        Some(100)
    );
}

#[tokio::test]
async fn apply_config_rejects_rules_in_overlay() {
    use conduit_proto::config::RulesConfig;

    let (snapshots, effective, configurator, tracing, base_dir) = support::minimal_control_setup();
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

    let mut client = ConduitControlClient::connect(format!("http://{local_addr}"))
        .await
        .expect("connect");

    let overlay = runtime_to_control(RuntimeConfig {
        schema_version: 1,
        rules: Some(RulesConfig {
            match_mode: "first_match".into(),
            rules: vec![],
        }),
        ..Default::default()
    });

    let apply = client
        .apply_config(ApplyConfigRequest {
            overlay: Some(overlay),
            mode: OverlayApplyMode::Merge.into(),
        })
        .await
        .expect("apply rpc")
        .into_inner();
    assert!(!apply.ok);
    assert!(apply.errors.iter().any(|e| e.contains("rules")));
    assert_eq!(snapshots.generation(), gen0);
}
