//! Integration tests for OTLP HTTP/HTTPS metrics push.

use conduit_config::{load_yaml, validate};
use conduit_core::snapshot::RuntimeSnapshot;
use conduit_events::EventHub;
use conduit_metrics::{compile_from_config, push_metrics_once, MetricsHub, OtelPushSettings};
use http_body_util::Full;
use hyper::body::Bytes;
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::{Method, Request, Response, StatusCode};
use hyper_util::rt::TokioIo;
use rcgen::{generate_simple_self_signed, CertifiedKey};
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use rustls::ServerConfig;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio_rustls::TlsAcceptor;

fn metrics_hub() -> Arc<MetricsHub> {
    let yaml = include_str!("../../../tests/fixtures/config/with-metrics-prometheus.yaml");
    let cfg = load_yaml(yaml).unwrap();
    Arc::new(MetricsHub::from_config(&cfg))
}

fn observation_hub() -> Arc<EventHub> {
    let yaml = include_str!("../../../tests/fixtures/config/with-metrics-prometheus.yaml");
    let cfg = load_yaml(yaml).unwrap();
    let snap = RuntimeSnapshot::from_config(cfg);
    Arc::new(EventHub::from_compiled(&snap.events))
}

async fn serve_http_once(
    listener: TcpListener,
    path: &'static str,
    hits: Arc<AtomicUsize>,
) -> std::io::Result<()> {
    let (stream, _) = listener.accept().await?;
    let io = TokioIo::new(stream);
    let svc = service_fn(move |req: Request<hyper::body::Incoming>| {
        let hits = hits.clone();
        async move {
            if req.method() == Method::POST && req.uri().path() == path {
                hits.fetch_add(1, Ordering::SeqCst);
                Ok::<_, std::convert::Infallible>(
                    Response::builder()
                        .status(StatusCode::OK)
                        .body(Full::new(Bytes::new()))
                        .unwrap(),
                )
            } else {
                Ok(Response::builder()
                    .status(StatusCode::NOT_FOUND)
                    .body(Full::new(Bytes::new()))
                    .unwrap())
            }
        }
    });
    let _ = http1::Builder::new().serve_connection(io, svc).await;
    Ok(())
}

fn tls_acceptor() -> TlsAcceptor {
    let CertifiedKey { cert, key_pair } =
        generate_simple_self_signed(vec!["localhost".into()]).expect("self-signed cert");
    let cert_der = CertificateDer::from(cert.der().to_vec());
    let key_der = PrivateKeyDer::Pkcs8(key_pair.serialize_der().into());
    let config = ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(vec![cert_der], key_der)
        .expect("tls server config");
    TlsAcceptor::from(Arc::new(config))
}

async fn serve_https_once(
    listener: TcpListener,
    acceptor: TlsAcceptor,
    path: &'static str,
    hits: Arc<AtomicUsize>,
) -> std::io::Result<()> {
    let (stream, _) = listener.accept().await?;
    let tls = acceptor.accept(stream).await?;
    let io = TokioIo::new(tls);
    let svc = service_fn(move |req: Request<hyper::body::Incoming>| {
        let hits = hits.clone();
        async move {
            if req.method() == Method::POST && req.uri().path() == path {
                hits.fetch_add(1, Ordering::SeqCst);
                Ok::<_, std::convert::Infallible>(
                    Response::builder()
                        .status(StatusCode::OK)
                        .body(Full::new(Bytes::new()))
                        .unwrap(),
                )
            } else {
                Ok(Response::builder()
                    .status(StatusCode::NOT_FOUND)
                    .body(Full::new(Bytes::new()))
                    .unwrap())
            }
        }
    });
    let _ = http1::Builder::new().serve_connection(io, svc).await;
    Ok(())
}

#[tokio::test]
async fn otel_push_http_succeeds() {
    let hits = Arc::new(AtomicUsize::new(0));
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr: SocketAddr = listener.local_addr().unwrap();
    let path = "/v1/metrics";

    let server_hits = hits.clone();
    let server = tokio::spawn(async move { serve_http_once(listener, path, server_hits).await });

    let hub = metrics_hub();
    let observation = observation_hub();
    let settings = OtelPushSettings {
        endpoint: format!("http://{addr}{path}"),
        push_interval_ms: 15_000,
        resource_attributes: vec![],
        allow_invalid_certs: false,
        headers: vec![],
    };

    push_metrics_once(hub.as_ref(), observation.as_ref(), &settings)
        .await
        .expect("http otel push");

    assert_eq!(hits.load(Ordering::SeqCst), 1);
    let _ = server.await;
}

#[tokio::test]
async fn otel_push_https_self_signed_fails_without_allow_invalid_certs() {
    let hits = Arc::new(AtomicUsize::new(0));
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr: SocketAddr = listener.local_addr().unwrap();
    let path = "/v1/metrics";
    let acceptor = tls_acceptor();

    let server_hits = hits.clone();
    let server =
        tokio::spawn(async move { serve_https_once(listener, acceptor, path, server_hits).await });

    let hub = metrics_hub();
    let observation = observation_hub();
    let settings = OtelPushSettings {
        endpoint: format!("https://{addr}{path}"),
        push_interval_ms: 15_000,
        resource_attributes: vec![],
        allow_invalid_certs: false,
        headers: vec![],
    };

    let err = push_metrics_once(hub.as_ref(), observation.as_ref(), &settings)
        .await
        .expect_err("tls verify should fail");
    assert!(!err.is_empty(), "expected non-empty push error");
    assert_eq!(hits.load(Ordering::SeqCst), 0);
    let _ = server.await;
}

#[tokio::test]
async fn otel_push_https_self_signed_succeeds_with_allow_invalid_certs() {
    let hits = Arc::new(AtomicUsize::new(0));
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr: SocketAddr = listener.local_addr().unwrap();
    let path = "/v1/metrics";
    let acceptor = tls_acceptor();

    let server_hits = hits.clone();
    let server =
        tokio::spawn(async move { serve_https_once(listener, acceptor, path, server_hits).await });

    let hub = metrics_hub();
    let observation = observation_hub();
    let settings = OtelPushSettings {
        endpoint: format!("https://{addr}{path}"),
        push_interval_ms: 15_000,
        resource_attributes: vec![],
        allow_invalid_certs: true,
        headers: vec![],
    };

    push_metrics_once(hub.as_ref(), observation.as_ref(), &settings)
        .await
        .expect("https otel push with allow_invalid_certs");

    assert_eq!(hits.load(Ordering::SeqCst), 1);
    let _ = server.await;
}

#[test]
fn otel_config_compiles_allow_invalid_certs() {
    let yaml = include_str!("../../../tests/fixtures/config/with-metrics-otel.yaml");
    let cfg = load_yaml(yaml).unwrap();
    assert!(validate(&cfg).ok, "{:?}", validate(&cfg).errors);
    let (compiled, _) = compile_from_config(&cfg);
    assert!(compiled.otel_endpoint.is_some());
    assert!(!compiled.otel_allow_invalid_certs);
}

#[test]
fn otel_yaml_roundtrip_preserves_allow_invalid_certs() {
    let yaml = include_str!("../../../tests/fixtures/config/with-metrics-otel.yaml");
    let mut cfg = load_yaml(yaml).unwrap();
    cfg.metrics
        .as_mut()
        .unwrap()
        .otel
        .as_mut()
        .unwrap()
        .allow_invalid_certs = Some(true);
    let exported = conduit_config::export_yaml(&cfg).unwrap();
    let cfg2 = load_yaml(&exported).unwrap();
    let otel = cfg2.metrics.as_ref().unwrap().otel.as_ref().unwrap();
    assert_eq!(otel.allow_invalid_certs, Some(true));
}
