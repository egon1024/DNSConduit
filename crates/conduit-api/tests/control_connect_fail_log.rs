//! Control-plane TLS handshake failure emits a warn-level connection log.

use conduit_api::tls::prepare_control_tls;
use conduit_config::{load_yaml, EffectiveConfig};
use conduit_core::{spawn_configurator, ConfiguratorState, RuntimeSnapshot, SnapshotStore};
use conduit_metrics::TracingHub;
use conduit_proto::config::{ControlConfig, ControlTlsConfig};
use std::io::Write;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn tls_fixture(name: &str) -> PathBuf {
    workspace_root()
        .join("tests/fixtures/tls/grpc-primitives")
        .join(name)
}

struct LogBuf(Arc<Mutex<Vec<u8>>>);

impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for LogBuf {
    type Writer = LogBufWriter;

    fn make_writer(&'a self) -> Self::Writer {
        LogBufWriter(self.0.clone())
    }
}

struct LogBufWriter(Arc<Mutex<Vec<u8>>>);

impl Write for LogBufWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[tokio::test]
async fn plaintext_client_to_tls_server_logs_warn() {
    let buf = Arc::new(Mutex::new(Vec::new()));
    let subscriber = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::WARN)
        .with_writer(LogBuf(buf.clone()))
        .with_ansi(false)
        .finish();
    let _guard = tracing::subscriber::set_default(subscriber);

    let yaml = include_str!("../../../tests/fixtures/config/minimal.yaml");
    let mut file_cfg = load_yaml(yaml).expect("parse");
    file_cfg.control = Some(ControlConfig {
        listen_address: "127.0.0.1:0".into(),
        reflection_enabled: false,
        api_keys: vec![],
        tls: Some(ControlTlsConfig {
            cert_path: tls_fixture("server.pem").display().to_string(),
            key_path: tls_fixture("server-key.pem").display().to_string(),
            client_ca_path: String::new(),
        }),
    });

    prepare_control_tls(
        file_cfg.control.as_ref().unwrap().tls.as_ref().unwrap(),
        None,
    )
    .expect("tls");

    let base_dir = Some(workspace_root().join("tests/fixtures/config"));
    let config_path = workspace_root().join("tests/fixtures/config/minimal.yaml");
    let snapshots = Arc::new(SnapshotStore::new(RuntimeSnapshot::from_config_with_base(
        file_cfg.clone(),
        base_dir.as_deref(),
    )));
    let tracing_hub = Arc::new(TracingHub::from_config(&file_cfg));
    let effective = Arc::new(Mutex::new(EffectiveConfig::new(file_cfg)));
    let state = ConfiguratorState {
        config_path,
        base_dir: base_dir.clone(),
        metrics_hub: None,
        export_controller: None,
        events: None,
    };
    let configurator = spawn_configurator(snapshots.clone(), effective.clone(), state).handle();

    let addr: SocketAddr = "127.0.0.1:0".parse().unwrap();
    let local = conduit_api::serve_on_listener(
        addr,
        snapshots,
        effective,
        configurator,
        tracing_hub,
        base_dir,
    )
    .await
    .expect("serve");

    // Plain TCP write (no TLS) — server handshake must fail and warn.
    let mut stream = std::net::TcpStream::connect(local).expect("connect");
    stream
        .set_read_timeout(Some(std::time::Duration::from_millis(200)))
        .ok();
    stream.write_all(b"not-a-tls-client-hello").ok();
    let _ = stream.shutdown(std::net::Shutdown::Both);
    drop(stream);

    tokio::time::sleep(std::time::Duration::from_millis(300)).await;

    let logged = String::from_utf8_lossy(&buf.lock().unwrap()).to_string();
    assert!(
        logged.contains("control plane connection failed"),
        "missing warn log, got: {logged}"
    );
    assert!(
        logged.contains("tls=true") || logged.contains("tls: true"),
        "missing tls=true, got: {logged}"
    );
}
