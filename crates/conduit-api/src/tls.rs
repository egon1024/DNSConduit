//! TLS / mTLS for the control gRPC server.

use conduit_proto::config::ControlTlsConfig;
use std::fs;
use std::path::Path;
use tonic::transport::{Certificate, Identity, ServerTlsConfig};

pub fn server_tls_config(tls: &ControlTlsConfig) -> anyhow::Result<ServerTlsConfig> {
    let cert_path = Path::new(&tls.cert_path);
    let key_path = Path::new(&tls.key_path);
    let cert = fs::read(cert_path)
        .map_err(|e| anyhow::anyhow!("reading TLS cert {:?}: {e}", cert_path))?;
    let key =
        fs::read(key_path).map_err(|e| anyhow::anyhow!("reading TLS key {:?}: {e}", key_path))?;
    let identity = Identity::from_pem(cert, key);
    let mut cfg = ServerTlsConfig::new().identity(identity);
    if !tls.client_ca_path.is_empty() {
        let ca_path = Path::new(&tls.client_ca_path);
        let ca = fs::read(ca_path)
            .map_err(|e| anyhow::anyhow!("reading client CA {:?}: {e}", ca_path))?;
        cfg = cfg.client_ca_root(Certificate::from_pem(ca));
    }
    Ok(cfg)
}
