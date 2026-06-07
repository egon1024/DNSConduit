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

pub fn spawn_prometheus_server(
    listen: SocketAddr,
    path: String,
    hub: Arc<MetricsHub>,
    observation: Arc<EventHub>,
) -> PrometheusServerHandle {
    let (shutdown_tx, mut shutdown_rx) = tokio::sync::oneshot::channel();
    let join = tokio::spawn(async move {
        let listener = match TcpListener::bind(listen).await {
            Ok(l) => l,
            Err(e) => {
                tracing::error!(%listen, error = %e, "prometheus metrics bind failed");
                return;
            }
        };
        tracing::info!(%listen, %path, "prometheus metrics listening");
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
        tracing::debug!(%listen, "prometheus metrics stopped");
    });
    PrometheusServerHandle::new(shutdown_tx, join)
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
