//! Prometheus HTTP scrape listener (off worker threads).

use crate::export::render_prometheus;
use crate::task::PrometheusServerHandle;
use crate::MetricsHub;
use conduit_events::EventHub;
use http_body_util::Full;
use hyper::body::Bytes;
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::{Request, Response, StatusCode};
use hyper_util::rt::TokioIo;
use std::convert::Infallible;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::TcpListener;

/// Spawn a Prometheus HTTP scrape server by binding to the given address.
///
/// This is the original API: binds inside the spawned task. Bind failure is
/// logged but does not fail the caller (process startup continues).
///
/// For hot-rebind support, use [`PrometheusServer::spawn_with_listener`] with
/// a pre-bound `TcpListener` so bind failures can be detected before snapshot
/// install.
pub fn spawn_prometheus_server(
    listen: SocketAddr,
    path: String,
    hub: Arc<MetricsHub>,
    observation: Arc<EventHub>,
) -> PrometheusServerHandle {
    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel();
    let join = tokio::spawn(async move {
        let listener = match TcpListener::bind(listen).await {
            Ok(l) => l,
            Err(e) => {
                tracing::error!(%listen, error = %e, "prometheus metrics bind failed");
                return;
            }
        };
        tracing::info!(%listen, %path, "prometheus metrics listening");
        serve_loop(listener, path, hub, observation, shutdown_rx).await;
    });
    PrometheusServerHandle::new(shutdown_tx, join)
}

/// Prometheus server with support for spawning from a pre-bound listener.
///
/// Used by `MetricsExportController` for hot-rebind: the controller pre-binds
/// the new address in `prepare()`, and if successful, passes the listener to
/// `spawn_with_listener()` in `commit()`.
pub struct PrometheusServer;

impl PrometheusServer {
    /// Spawn a Prometheus HTTP scrape server using an already-bound `TcpListener`.
    ///
    /// Use this for hot-rebind: bind first (failing fast if the port is in use),
    /// then pass the listener here after the snapshot install succeeds.
    pub fn spawn_with_listener(
        listener: TcpListener,
        path: String,
        hub: Arc<MetricsHub>,
        observation: Arc<EventHub>,
    ) -> PrometheusServerHandle {
        let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel();
        let addr = listener.local_addr().ok();
        let join = tokio::spawn(async move {
            if let Some(addr) = addr {
                tracing::debug!(%addr, %path, "prometheus server starting with pre-bound listener");
            }
            serve_loop(listener, path, hub, observation, shutdown_rx).await;
        });
        PrometheusServerHandle::new(shutdown_tx, join)
    }
}

/// Shared serve loop used by both spawn variants.
async fn serve_loop(
    listener: TcpListener,
    path: String,
    hub: Arc<MetricsHub>,
    observation: Arc<EventHub>,
    mut shutdown_rx: tokio::sync::oneshot::Receiver<()>,
) {
    let listen = listener.local_addr().ok();
    loop {
        tokio::select! {
            _ = &mut shutdown_rx => break,
            accept = listener.accept() => {
                let Ok((stream, _)) = accept else {
                    continue;
                };
                let hub = hub.clone();
                let observation = observation.clone();
                let path = path.clone();
                tokio::spawn(async move {
                    let io = TokioIo::new(stream);
                    let svc = service_fn(move |req: Request<hyper::body::Incoming>| {
                        let hub = hub.clone();
                        let observation = observation.clone();
                        let path = path.clone();
                        async move {
                            Ok::<_, Infallible>(handle_metrics(req, &path, &hub, &observation))
                        }
                    });
                    if http1::Builder::new()
                        .serve_connection(io, svc)
                        .await
                        .is_err()
                    {}
                });
            }
        }
    }
    if let Some(addr) = listen {
        tracing::debug!(%addr, "prometheus metrics stopped");
    }
}

fn handle_metrics(
    req: Request<hyper::body::Incoming>,
    path: &str,
    hub: &MetricsHub,
    observation: &EventHub,
) -> Response<Full<Bytes>> {
    if req.method() != hyper::Method::GET {
        return Response::builder()
            .status(StatusCode::METHOD_NOT_ALLOWED)
            .body(Full::new(Bytes::new()))
            .unwrap();
    }
    if req.uri().path() != path {
        return Response::builder()
            .status(StatusCode::NOT_FOUND)
            .body(Full::new(Bytes::new()))
            .unwrap();
    }
    let obs = observation.sink_metrics_snapshot();
    let body = render_prometheus(hub, &obs);
    Response::builder()
        .status(StatusCode::OK)
        .header("Content-Type", "text/plain; version=0.0.4; charset=utf-8")
        .body(Full::new(Bytes::from(body)))
        .unwrap()
}
