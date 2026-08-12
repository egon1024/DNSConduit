//! TLS / mTLS for the control gRPC server.

use conduit_config::resolve_config_path;
use conduit_proto::config::ControlTlsConfig;
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use rustls::server::WebPkiClientVerifier;
use rustls::{RootCertStore, ServerConfig};
use std::fs;
use std::path::Path;
use std::sync::Arc;
use tokio_rustls::TlsAcceptor;

/// Prepared control-plane TLS acceptor (handshake performed in our accept loop).
#[derive(Clone)]
pub struct PreparedControlTls {
    pub acceptor: TlsAcceptor,
}

/// Load control TLS material, emit the startup info line, and build a rustls acceptor.
pub fn prepare_control_tls(
    tls: &ControlTlsConfig,
    base_dir: Option<&Path>,
) -> anyhow::Result<PreparedControlTls> {
    let cert_path = resolve_config_path(base_dir, &tls.cert_path);
    let key_path = resolve_config_path(base_dir, &tls.key_path);
    let cert_pem = fs::read(&cert_path)
        .map_err(|e| anyhow::anyhow!("reading TLS cert {:?}: {e}", cert_path))?;
    let key_pem =
        fs::read(&key_path).map_err(|e| anyhow::anyhow!("reading TLS key {:?}: {e}", key_path))?;

    let client_ca_path = if tls.client_ca_path.is_empty() {
        None
    } else {
        let ca_path = resolve_config_path(base_dir, &tls.client_ca_path);
        // Ensure the CA is readable before building the verifier.
        let _ = fs::read(&ca_path)
            .map_err(|e| anyhow::anyhow!("reading client CA {:?}: {e}", ca_path))?;
        Some(ca_path)
    };

    log_control_tls_enabled(&cert_path, &key_path, client_ca_path.as_deref(), &cert_pem);

    let certs = load_certs(&cert_pem)?;
    let key = load_private_key(&key_pem)?;

    let builder = ServerConfig::builder();
    let builder = if let Some(ref ca_path) = client_ca_path {
        let ca_pem = fs::read(ca_path)
            .map_err(|e| anyhow::anyhow!("reading client CA {:?}: {e}", ca_path))?;
        let mut roots = RootCertStore::empty();
        for cert in load_certs(&ca_pem)? {
            roots
                .add(cert)
                .map_err(|e| anyhow::anyhow!("adding client CA: {e}"))?;
        }
        let verifier = WebPkiClientVerifier::builder(Arc::new(roots))
            .build()
            .map_err(|e| anyhow::anyhow!("building client cert verifier: {e}"))?;
        builder.with_client_cert_verifier(verifier)
    } else {
        builder.with_no_client_auth()
    };

    let mut config = builder
        .with_single_cert(certs, key)
        .map_err(|e| anyhow::anyhow!("building TLS server config: {e}"))?;
    config.alpn_protocols = vec![b"h2".to_vec()];

    Ok(PreparedControlTls {
        acceptor: TlsAcceptor::from(Arc::new(config)),
    })
}

/// Compatibility alias used by path-resolution tests.
pub fn server_tls_config(
    tls: &ControlTlsConfig,
    base_dir: Option<&Path>,
) -> anyhow::Result<PreparedControlTls> {
    prepare_control_tls(tls, base_dir)
}

fn load_certs(pem: &[u8]) -> anyhow::Result<Vec<CertificateDer<'static>>> {
    let mut reader = std::io::Cursor::new(pem);
    let certs: Vec<CertificateDer<'static>> = rustls_pemfile::certs(&mut reader)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| anyhow::anyhow!("parsing certificate PEM: {e}"))?;
    if certs.is_empty() {
        anyhow::bail!("no certificates in PEM");
    }
    Ok(certs)
}

fn load_private_key(pem: &[u8]) -> anyhow::Result<PrivateKeyDer<'static>> {
    let mut reader = std::io::Cursor::new(pem);
    rustls_pemfile::private_key(&mut reader)
        .map_err(|e| anyhow::anyhow!("parsing private key PEM: {e}"))?
        .ok_or_else(|| anyhow::anyhow!("no private key in PEM"))
}

fn log_control_tls_enabled(
    cert_path: &Path,
    key_path: &Path,
    client_ca_path: Option<&Path>,
    cert_pem: &[u8],
) {
    match leaf_cert_basic_info(cert_pem) {
        Ok(info) => {
            tracing::info!(
                cert_path = %cert_path.display(),
                key_path = %key_path.display(),
                client_ca_path = client_ca_path.map(|p| p.display().to_string()),
                mtls = client_ca_path.is_some(),
                subject = %info.subject,
                not_before = %info.not_before,
                not_after = %info.not_after,
                san = %info.san,
                "control plane TLS enabled"
            );
        }
        Err(e) => {
            tracing::info!(
                cert_path = %cert_path.display(),
                key_path = %key_path.display(),
                client_ca_path = client_ca_path.map(|p| p.display().to_string()),
                mtls = client_ca_path.is_some(),
                cert_parse_error = %e,
                "control plane TLS enabled"
            );
        }
    }
}

struct LeafCertBasicInfo {
    subject: String,
    not_before: String,
    not_after: String,
    san: String,
}

fn leaf_cert_basic_info(cert_pem: &[u8]) -> anyhow::Result<LeafCertBasicInfo> {
    let mut reader = std::io::Cursor::new(cert_pem);
    let der = rustls_pemfile::certs(&mut reader)
        .next()
        .ok_or_else(|| anyhow::anyhow!("no certificates in PEM"))?
        .map_err(|e| anyhow::anyhow!("parsing certificate PEM: {e}"))?;
    let (_, cert) = x509_parser::parse_x509_certificate(der.as_ref())
        .map_err(|e| anyhow::anyhow!("parsing X.509 certificate: {e}"))?;
    let san = match cert.subject_alternative_name() {
        Ok(Some(ext)) => ext
            .value
            .general_names
            .iter()
            .map(format_general_name)
            .collect::<Vec<_>>()
            .join(", "),
        Ok(None) | Err(_) => String::new(),
    };
    Ok(LeafCertBasicInfo {
        subject: cert.subject().to_string(),
        not_before: cert.validity().not_before.to_string(),
        not_after: cert.validity().not_after.to_string(),
        san: if san.is_empty() { "-".into() } else { san },
    })
}

fn format_general_name(name: &x509_parser::extensions::GeneralName<'_>) -> String {
    use x509_parser::extensions::GeneralName;
    match name {
        GeneralName::DNSName(dns) => format!("DNS:{dns}"),
        GeneralName::IPAddress(bytes) if bytes.len() == 4 => {
            format!("IP:{}.{}.{}.{}", bytes[0], bytes[1], bytes[2], bytes[3])
        }
        GeneralName::IPAddress(bytes) if bytes.len() == 16 => {
            let mut arr = [0u8; 16];
            arr.copy_from_slice(bytes);
            format!("IP:{}", std::net::Ipv6Addr::from(arr))
        }
        GeneralName::IPAddress(bytes) => format!("IP:{}", hex_bytes(bytes)),
        GeneralName::RFC822Name(mail) => format!("email:{mail}"),
        GeneralName::URI(uri) => format!("URI:{uri}"),
        other => other.to_string(),
    }
}

fn hex_bytes(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<Vec<_>>()
        .join(":")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::{Path, PathBuf};

    fn fixture(name: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures/tls/grpc-primitives")
            .join(name)
    }

    #[test]
    fn resolves_relative_tls_paths_against_base_dir() {
        let base = Path::new("/etc/conduit");
        let tls = ControlTlsConfig {
            cert_path: "tls/cert.pem".into(),
            key_path: "tls/key.pem".into(),
            client_ca_path: "tls/ca.pem".into(),
        };
        // Exercise path resolution via read errors (files need not exist).
        let err = prepare_control_tls(&tls, Some(base))
            .err()
            .expect("missing cert should fail");
        let msg = err.to_string();
        assert!(msg.contains("/etc/conduit/tls/cert.pem"));
    }

    #[test]
    fn leaf_cert_basic_info_reads_lab_server_fixture() {
        let pem = fs::read(fixture("server.pem")).expect("fixture");
        let info = leaf_cert_basic_info(&pem).expect("parse");
        assert!(
            info.subject.contains("localhost"),
            "subject={}",
            info.subject
        );
        assert!(
            info.san.contains("127.0.2.1") || info.san.contains("localhost"),
            "san={}",
            info.san
        );
        assert!(!info.not_after.is_empty());
    }

    #[test]
    fn prepare_control_tls_loads_lab_fixtures() {
        let tls = ControlTlsConfig {
            cert_path: fixture("server.pem").display().to_string(),
            key_path: fixture("server-key.pem").display().to_string(),
            client_ca_path: String::new(),
        };
        prepare_control_tls(&tls, None).expect("prepare");
    }
}
