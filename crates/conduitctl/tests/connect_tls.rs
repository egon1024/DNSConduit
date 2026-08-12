//! TLS / plaintext connect integration tests for the shared helper.

use conduit_config::{load_yaml, EffectiveConfig};
use conduit_core::{spawn_configurator, ConfiguratorState, RuntimeSnapshot, SnapshotStore};
use conduit_metrics::TracingHub;
use conduit_proto::config::{ControlConfig, ControlTlsConfig};
use conduit_proto::control::conduit_control_client::ConduitControlClient;
use conduit_proto::control::ExportConfigRequest;
use conduitctl::{connect_channel, resolve_connect, ConnectCliOverrides, ResolvedConnect};
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

type ControlSetup = (
    Arc<SnapshotStore>,
    Arc<Mutex<EffectiveConfig>>,
    conduit_core::ConfiguratorHandle,
    Arc<TracingHub>,
    Option<PathBuf>,
);

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn tls_fixture(name: &str) -> PathBuf {
    workspace_root()
        .join("tests/fixtures/tls/grpc-primitives")
        .join(name)
}

fn fixture_rel(name: &str) -> String {
    format!("../../fixtures/tls/grpc-primitives/{name}")
}

fn control_setup_with_tls(cert: &str, key: &str, client_ca: Option<&str>) -> ControlSetup {
    let yaml = include_str!("../../../tests/fixtures/config/minimal.yaml");
    let mut file_cfg = load_yaml(yaml).expect("parse");
    let mut control = file_cfg.control.unwrap_or_else(|| ControlConfig {
        listen_address: "127.0.0.1:0".into(),
        reflection_enabled: false,
        api_keys: vec![],
        tls: None,
    });
    control.tls = Some(ControlTlsConfig {
        cert_path: fixture_rel(cert),
        key_path: fixture_rel(key),
        client_ca_path: client_ca.map(fixture_rel).unwrap_or_default(),
    });
    file_cfg.control = Some(control);

    let base_dir = Some(workspace_root().join("tests/fixtures/config"));
    let config_path = workspace_root().join("tests/fixtures/config/minimal.yaml");
    let snapshots = Arc::new(SnapshotStore::new(RuntimeSnapshot::from_config_with_base(
        file_cfg.clone(),
        base_dir.as_deref(),
    )));
    let tracing = Arc::new(TracingHub::from_config(&file_cfg));
    let effective = Arc::new(Mutex::new(EffectiveConfig::new(file_cfg)));
    let state = ConfiguratorState {
        config_path,
        base_dir: base_dir.clone(),
        metrics_hub: None,
        export_controller: None,
        events: None,
    };
    let configurator = spawn_configurator(snapshots.clone(), effective.clone(), state).handle();
    (snapshots, effective, configurator, tracing, base_dir)
}

fn plaintext_setup() -> ControlSetup {
    let yaml = include_str!("../../../tests/fixtures/config/minimal.yaml");
    let file_cfg = load_yaml(yaml).expect("parse");
    let base_dir = Some(workspace_root().join("tests/fixtures/config"));
    let config_path = workspace_root().join("tests/fixtures/config/minimal.yaml");
    let snapshots = Arc::new(SnapshotStore::new(RuntimeSnapshot::from_config_with_base(
        file_cfg.clone(),
        base_dir.as_deref(),
    )));
    let tracing = Arc::new(TracingHub::from_config(&file_cfg));
    let effective = Arc::new(Mutex::new(EffectiveConfig::new(file_cfg)));
    let state = ConfiguratorState {
        config_path,
        base_dir: base_dir.clone(),
        metrics_hub: None,
        export_controller: None,
        events: None,
    };
    let configurator = spawn_configurator(snapshots.clone(), effective.clone(), state).handle();
    (snapshots, effective, configurator, tracing, base_dir)
}

async fn start_server(
    snapshots: Arc<SnapshotStore>,
    effective: Arc<Mutex<EffectiveConfig>>,
    configurator: conduit_core::ConfiguratorHandle,
    tracing: Arc<TracingHub>,
    base_dir: Option<PathBuf>,
) -> SocketAddr {
    let addr: SocketAddr = "127.0.0.1:0".parse().expect("parse");
    conduit_api::serve_on_listener(addr, snapshots, effective, configurator, tracing, base_dir)
        .await
        .expect("start server")
}

async fn export_ok(resolved: &ResolvedConnect) {
    let channel = connect_channel(resolved).await.expect("connect");
    let mut client = ConduitControlClient::new(channel);
    let resp = client
        .export_config(ExportConfigRequest {
            format: "yaml".into(),
        })
        .await
        .expect("export")
        .into_inner();
    assert!(!resp.body.is_empty());
}

#[tokio::test]
async fn plaintext_default_connect_works() {
    let (snapshots, effective, configurator, tracing, base_dir) = plaintext_setup();
    let addr = start_server(snapshots, effective, configurator, tracing, base_dir).await;
    let resolved = ResolvedConnect {
        endpoint: format!("http://{addr}"),
        api_key: None,
        tls_ca: None,
        tls_cert: None,
        tls_key: None,
        insecure_skip_verify: false,
        client_config_path: PathBuf::from("/tmp/missing-conduitctl.yaml"),
        client_config_loaded: false,
    };
    export_ok(&resolved).await;
}

#[tokio::test]
async fn https_verify_with_custom_ca_succeeds() {
    let (snapshots, effective, configurator, tracing, base_dir) =
        control_setup_with_tls("server.pem", "server-key.pem", None);
    let addr = start_server(snapshots, effective, configurator, tracing, base_dir).await;
    let resolved = ResolvedConnect {
        endpoint: format!("https://{addr}"),
        api_key: None,
        tls_ca: Some(tls_fixture("ca.pem")),
        tls_cert: None,
        tls_key: None,
        insecure_skip_verify: false,
        client_config_path: PathBuf::from("/tmp/missing-conduitctl.yaml"),
        client_config_loaded: false,
    };
    export_ok(&resolved).await;
}

#[tokio::test]
async fn https_hostname_mismatch_fails() {
    let (snapshots, effective, configurator, tracing, base_dir) =
        control_setup_with_tls("wrong-host.pem", "wrong-host-key.pem", None);
    let addr = start_server(snapshots, effective, configurator, tracing, base_dir).await;
    // Connect by IP; cert only has DNS:wrong.example → hostname/IP mismatch.
    let resolved = ResolvedConnect {
        endpoint: format!("https://{addr}"),
        api_key: None,
        tls_ca: Some(tls_fixture("ca.pem")),
        tls_cert: None,
        tls_key: None,
        insecure_skip_verify: false,
        client_config_path: PathBuf::from("/tmp/missing-conduitctl.yaml"),
        client_config_loaded: false,
    };
    let err = connect_channel(&resolved)
        .await
        .expect_err("hostname mismatch");
    let msg = format!("{err:#}");
    assert!(
        msg.to_ascii_lowercase().contains("certificate")
            || msg.to_ascii_lowercase().contains("tls")
            || msg.to_ascii_lowercase().contains("handshake")
            || msg.to_ascii_lowercase().contains("invalid"),
        "unexpected error: {msg}"
    );
}

#[tokio::test]
async fn https_untrusted_chain_fails() {
    let (snapshots, effective, configurator, tracing, base_dir) =
        control_setup_with_tls("selfsigned.pem", "selfsigned-key.pem", None);
    let addr = start_server(snapshots, effective, configurator, tracing, base_dir).await;
    let resolved = ResolvedConnect {
        endpoint: format!("https://{addr}"),
        api_key: None,
        tls_ca: None, // platform roots only — self-signed not trusted
        tls_cert: None,
        tls_key: None,
        insecure_skip_verify: false,
        client_config_path: PathBuf::from("/tmp/missing-conduitctl.yaml"),
        client_config_loaded: false,
    };
    let err = connect_channel(&resolved).await.expect_err("untrusted");
    let msg = format!("{err:#}");
    assert!(
        msg.to_ascii_lowercase().contains("certificate")
            || msg.to_ascii_lowercase().contains("tls")
            || msg.to_ascii_lowercase().contains("handshake")
            || msg.to_ascii_lowercase().contains("invalid"),
        "unexpected error: {msg}"
    );
}

#[tokio::test]
async fn https_skip_verify_self_signed_succeeds() {
    let (snapshots, effective, configurator, tracing, base_dir) =
        control_setup_with_tls("selfsigned.pem", "selfsigned-key.pem", None);
    let addr = start_server(snapshots, effective, configurator, tracing, base_dir).await;
    let resolved = ResolvedConnect {
        endpoint: format!("https://{addr}"),
        api_key: None,
        tls_ca: None,
        tls_cert: None,
        tls_key: None,
        insecure_skip_verify: true,
        client_config_path: PathBuf::from("/tmp/missing-conduitctl.yaml"),
        client_config_loaded: false,
    };
    export_ok(&resolved).await;
}

#[tokio::test]
async fn https_mtls_with_client_identity_succeeds() {
    let (snapshots, effective, configurator, tracing, base_dir) =
        control_setup_with_tls("server.pem", "server-key.pem", Some("ca.pem"));
    let addr = start_server(snapshots, effective, configurator, tracing, base_dir).await;
    let resolved = ResolvedConnect {
        endpoint: format!("https://{addr}"),
        api_key: None,
        tls_ca: Some(tls_fixture("ca.pem")),
        tls_cert: Some(tls_fixture("client.pem")),
        tls_key: Some(tls_fixture("client-key.pem")),
        insecure_skip_verify: false,
        client_config_path: PathBuf::from("/tmp/missing-conduitctl.yaml"),
        client_config_loaded: false,
    };
    export_ok(&resolved).await;
}

#[tokio::test]
async fn resolve_from_yaml_file_feeds_connect() {
    let (snapshots, effective, configurator, tracing, base_dir) =
        control_setup_with_tls("server.pem", "server-key.pem", None);
    let addr = start_server(snapshots, effective, configurator, tracing, base_dir).await;

    let dir = tempfile::tempdir().unwrap();
    let cfg_path = dir.path().join("conduitctl.yaml");
    std::fs::write(
        &cfg_path,
        format!(
            "endpoint: https://{addr}\ntls:\n  ca: {}\n",
            tls_fixture("ca.pem").display()
        ),
    )
    .unwrap();

    let resolved = resolve_connect(&ConnectCliOverrides {
        config_path: Some(cfg_path),
        ..Default::default()
    })
    .unwrap();
    assert!(resolved.client_config_loaded);
    export_ok(&resolved).await;
}

#[test]
fn offline_validate_does_not_require_client_file() {
    // Exercise the same validate path as the CLI without a client config.
    let path = workspace_root().join("tests/fixtures/config/minimal.yaml");
    assert!(path.is_file());
    let yaml = std::fs::read_to_string(&path).unwrap();
    let cfg = load_yaml(&yaml).unwrap();
    let v = conduit_config::validate(&cfg);
    assert!(v.ok, "{:?}", v.errors);
    let _ = Path::new("/"); // keep Path import used for clarity
}
