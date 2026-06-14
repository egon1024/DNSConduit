//! TLS / mTLS for the control gRPC server.

use conduit_config::resolve_config_path;
use conduit_proto::config::ControlTlsConfig;
use std::fs;
use std::path::Path;
use tonic::transport::{Certificate, Identity, ServerTlsConfig};

pub fn server_tls_config(
    tls: &ControlTlsConfig,
    base_dir: Option<&Path>,
) -> anyhow::Result<ServerTlsConfig> {
    let cert_path = resolve_config_path(base_dir, &tls.cert_path);
    let key_path = resolve_config_path(base_dir, &tls.key_path);
    let cert = fs::read(&cert_path)
        .map_err(|e| anyhow::anyhow!("reading TLS cert {:?}: {e}", cert_path))?;
    let key =
        fs::read(&key_path).map_err(|e| anyhow::anyhow!("reading TLS key {:?}: {e}", key_path))?;
    let identity = Identity::from_pem(cert, key);
    let mut cfg = ServerTlsConfig::new().identity(identity);
    if !tls.client_ca_path.is_empty() {
        let ca_path = resolve_config_path(base_dir, &tls.client_ca_path);
        let ca = fs::read(&ca_path)
            .map_err(|e| anyhow::anyhow!("reading client CA {:?}: {e}", ca_path))?;
        cfg = cfg.client_ca_root(Certificate::from_pem(ca));
    }
    Ok(cfg)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn resolves_relative_tls_paths_against_base_dir() {
        let base = Path::new("/etc/conduit");
        let tls = ControlTlsConfig {
            cert_path: "tls/cert.pem".into(),
            key_path: "tls/key.pem".into(),
            client_ca_path: "tls/ca.pem".into(),
        };
        // Exercise path resolution via read errors (files need not exist).
        let err = server_tls_config(&tls, Some(base)).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("/etc/conduit/tls/cert.pem"));
    }
}
