//! Integration tests for control-plane TLS path resolution.

use conduit_api::tls::server_tls_config;
use conduit_config::resolve_config_path;
use conduit_proto::config::ControlTlsConfig;
use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;
use tempfile::TempDir;

/// These tests mutate process cwd; serialize them against each other.
static CWD_LOCK: Mutex<()> = Mutex::new(());

fn workspace_fixture(rel: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../")
        .join(rel)
}

#[test]
fn relative_tls_paths_resolve_against_config_directory() {
    let _guard = CWD_LOCK.lock().unwrap();
    let root = TempDir::new().unwrap();
    let config_dir = root.path().join("conduit");
    let tls_dir = config_dir.join("tls");
    fs::create_dir_all(&tls_dir).unwrap();

    let cert_src = workspace_fixture("tests/fixtures/tls/cert.pem");
    let key_src = workspace_fixture("tests/fixtures/tls/key.pem");
    fs::copy(cert_src, tls_dir.join("cert.pem")).unwrap();
    fs::copy(key_src, tls_dir.join("key.pem")).unwrap();

    let other = root.path().join("elsewhere");
    fs::create_dir_all(&other).unwrap();
    let original = std::env::current_dir().unwrap();
    std::env::set_current_dir(&other).unwrap();

    let tls = ControlTlsConfig {
        cert_path: "tls/cert.pem".into(),
        key_path: "tls/key.pem".into(),
        client_ca_path: String::new(),
    };
    let resolved_cert = resolve_config_path(Some(config_dir.as_path()), &tls.cert_path);
    assert_eq!(resolved_cert, config_dir.join("tls/cert.pem"));

    server_tls_config(&tls, Some(config_dir.as_path()))
        .expect("load TLS from config-relative paths");

    std::env::set_current_dir(original).unwrap();
}

#[test]
fn relative_tls_paths_fail_when_resolved_from_wrong_cwd_without_base_dir() {
    let _guard = CWD_LOCK.lock().unwrap();
    let root = TempDir::new().unwrap();
    let config_dir = root.path().join("conduit");
    let tls_dir = config_dir.join("tls");
    fs::create_dir_all(&tls_dir).unwrap();

    let cert_src = workspace_fixture("tests/fixtures/tls/cert.pem");
    let key_src = workspace_fixture("tests/fixtures/tls/key.pem");
    fs::copy(cert_src, tls_dir.join("cert.pem")).unwrap();
    fs::copy(key_src, tls_dir.join("key.pem")).unwrap();

    let other = root.path().join("elsewhere");
    fs::create_dir_all(&other).unwrap();
    let original = std::env::current_dir().unwrap();
    std::env::set_current_dir(&other).unwrap();

    let tls = ControlTlsConfig {
        cert_path: "tls/cert.pem".into(),
        key_path: "tls/key.pem".into(),
        client_ca_path: String::new(),
    };
    let err = match server_tls_config(&tls, None) {
        Ok(_) => panic!("cwd must not find tls under config dir"),
        Err(e) => e,
    };
    assert!(err.to_string().contains("reading TLS cert"), "{err}");

    std::env::set_current_dir(original).unwrap();
}
