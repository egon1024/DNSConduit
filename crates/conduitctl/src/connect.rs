//! Shared gRPC channel construction for `conduitctl` remote commands.

use crate::client_config::ResolvedConnect;
use anyhow::{anyhow, Context};
use http::Uri;
use hyper_util::rt::TokioIo;
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::pki_types::{CertificateDer, PrivateKeyDer, ServerName, UnixTime};
use rustls::{ClientConfig, DigitallySignedStruct, Error as TlsError, SignatureScheme};
use std::fs;
use std::path::Path;
use std::sync::Arc;
use tokio::net::TcpStream;
use tokio_rustls::TlsConnector;
use tonic::metadata::{Ascii, MetadataValue};
use tonic::transport::{Certificate, Channel, ClientTlsConfig, Endpoint, Identity};

/// Open a tonic channel using resolved connect settings.
pub async fn connect_channel(resolved: &ResolvedConnect) -> anyhow::Result<Channel> {
    let endpoint_uri = resolved
        .endpoint
        .parse::<Uri>()
        .with_context(|| format!("invalid control endpoint {:?}", resolved.endpoint))?;
    let scheme = endpoint_uri.scheme_str().unwrap_or("http");

    match scheme {
        "http" => Endpoint::from_shared(resolved.endpoint.clone())
            .context("invalid control endpoint")?
            .connect()
            .await
            .context("connect to control plane"),
        "https" => {
            if resolved.insecure_skip_verify {
                connect_https_insecure(resolved, &endpoint_uri).await
            } else {
                connect_https_verified(resolved, &endpoint_uri).await
            }
        }
        other => Err(anyhow!(
            "unsupported control endpoint scheme {other:?}; use http:// or https://"
        )),
    }
}

async fn connect_https_verified(resolved: &ResolvedConnect, uri: &Uri) -> anyhow::Result<Channel> {
    let host = uri
        .host()
        .ok_or_else(|| anyhow!("https endpoint missing host"))?
        .to_string();

    let mut tls = ClientTlsConfig::new().domain_name(host);
    if let Some(ref ca) = resolved.tls_ca {
        let pem = fs::read(ca).with_context(|| format!("reading TLS CA {:?}", ca))?;
        tls = tls.ca_certificate(Certificate::from_pem(pem));
    } else {
        tls = tls.with_enabled_roots();
    }
    if let (Some(cert), Some(key)) = (&resolved.tls_cert, &resolved.tls_key) {
        tls = tls.identity(load_identity(cert, key)?);
    }

    Endpoint::from_shared(resolved.endpoint.clone())
        .context("invalid control endpoint")?
        .tls_config(tls)
        .context("TLS client config")?
        .connect()
        .await
        .context("connect to control plane (TLS)")
}

async fn connect_https_insecure(resolved: &ResolvedConnect, uri: &Uri) -> anyhow::Result<Channel> {
    let host = uri
        .host()
        .ok_or_else(|| anyhow!("https endpoint missing host"))?
        .to_string();
    let port = uri.port_u16().unwrap_or(443);

    let builder = ClientConfig::builder()
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(SkipServerVerification));

    let config = if let (Some(cert), Some(key)) = (&resolved.tls_cert, &resolved.tls_key) {
        let (certs, key_der) = load_identity_der(cert, key)?;
        builder
            .with_client_auth_cert(certs, key_der)
            .context("client TLS identity")?
    } else {
        builder.with_no_client_auth()
    };

    let mut config = config;
    config.alpn_protocols = vec![b"h2".to_vec()];
    let tls = TlsConnector::from(Arc::new(config));
    let dial = format!("http://{host}:{port}");

    let connector = tower::service_fn(move |_: Uri| {
        let host = host.clone();
        let tls = tls.clone();
        async move {
            let tcp = TcpStream::connect((host.as_str(), port))
                .await
                .map_err(std::io::Error::other)?;
            let server_name = ServerName::try_from(host.clone())
                .map_err(|_| std::io::Error::other("invalid TLS server name"))?;
            let tls_stream = tls
                .connect(server_name, tcp)
                .await
                .map_err(std::io::Error::other)?;
            Ok::<_, std::io::Error>(TokioIo::new(tls_stream))
        }
    });

    // Use an http:// URI so tonic does not attempt a second TLS handshake on
    // top of the connector's already-encrypted stream (tonic 0.12).
    Endpoint::from_shared(dial)
        .context("invalid control endpoint")?
        .connect_with_connector(connector)
        .await
        .context("connect to control plane (TLS skip-verify)")
}

fn load_identity(cert: &Path, key: &Path) -> anyhow::Result<Identity> {
    let cert_pem = fs::read(cert).with_context(|| format!("reading TLS cert {:?}", cert))?;
    let key_pem = fs::read(key).with_context(|| format!("reading TLS key {:?}", key))?;
    Ok(Identity::from_pem(cert_pem, key_pem))
}

fn load_identity_der(
    cert: &Path,
    key: &Path,
) -> anyhow::Result<(Vec<CertificateDer<'static>>, PrivateKeyDer<'static>)> {
    let cert_pem = fs::read(cert).with_context(|| format!("reading TLS cert {:?}", cert))?;
    let key_pem = fs::read(key).with_context(|| format!("reading TLS key {:?}", key))?;

    let mut cert_reader = std::io::Cursor::new(cert_pem);
    let certs: Vec<CertificateDer<'static>> = rustls_pemfile::certs(&mut cert_reader)
        .collect::<Result<Vec<_>, _>>()
        .context("parsing client certificate PEM")?;
    if certs.is_empty() {
        anyhow::bail!("no certificates in {:?}", cert);
    }

    let mut key_reader = std::io::Cursor::new(key_pem);
    let key_der = rustls_pemfile::private_key(&mut key_reader)
        .context("parsing client private key PEM")?
        .ok_or_else(|| anyhow!("no private key in {:?}", key))?;

    Ok((certs, key_der))
}

/// Build `Authorization: Bearer …` metadata when an API key is configured.
pub fn auth_metadata(resolved: &ResolvedConnect) -> anyhow::Result<Option<MetadataValue<Ascii>>> {
    let Some(ref key) = resolved.api_key else {
        return Ok(None);
    };
    let value = format!("Bearer {key}");
    let meta = MetadataValue::try_from(value.as_str()).context("invalid API key for metadata")?;
    Ok(Some(meta))
}

pub fn with_auth<T>(
    resolved: &ResolvedConnect,
    mut request: tonic::Request<T>,
) -> anyhow::Result<tonic::Request<T>> {
    if let Some(meta) = auth_metadata(resolved)? {
        request.metadata_mut().insert("authorization", meta);
    }
    Ok(request)
}

#[derive(Debug)]
struct SkipServerVerification;

impl ServerCertVerifier for SkipServerVerification {
    fn verify_server_cert(
        &self,
        _end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, TlsError> {
        Ok(ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, TlsError> {
        Ok(HandshakeSignatureValid::assertion())
    }

    fn verify_tls13_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, TlsError> {
        Ok(HandshakeSignatureValid::assertion())
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        rustls::crypto::ring::default_provider()
            .signature_verification_algorithms
            .supported_schemes()
    }
}
