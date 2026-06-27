//! Control-plane gRPC access logging (phase 5).
//!
//! Logs RPC method, peer, requestor identity, status, and latency. Request/response
//! payloads are intentionally omitted; see plan deferrals for rich audit with redaction.

use crate::auth::requestor_label;
use conduit_core::SnapshotStore;
use http::{Request, Response};
use pin_project::pin_project;
use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::Instant;
use tonic::body::BoxBody;
use tonic::metadata::MetadataMap;
use tonic::transport::server::{TcpConnectInfo, TlsConnectInfo};
use tonic::{Code, GrpcMethod, Status};
use tower::{Layer, Service};

/// Emit the **application-level** outcome of a control RPC as its own log line,
/// separate from the transport-level `control rpc` line emitted by the access-log
/// middleware. Handlers whose verdict lives in the message body (`ok`/`errors`)
/// rather than the gRPC status call this so a rejected apply/validate/reload is
/// visible even though the transport status is `Ok`.
pub fn log_control_outcome(rpc: &str, ok: bool, errors: &[String]) {
    tracing::info!(
        rpc,
        outcome = if ok { "ok" } else { "rejected" },
        error_count = errors.len(),
        errors = ?errors.join("; "),
        "control rpc outcome"
    );
}

#[derive(Clone)]
pub struct AccessLogLayer {
    snapshots: std::sync::Arc<SnapshotStore>,
}

impl AccessLogLayer {
    pub fn new(snapshots: std::sync::Arc<SnapshotStore>) -> Self {
        Self { snapshots }
    }
}

impl<S> Layer<S> for AccessLogLayer {
    type Service = AccessLogService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        AccessLogService {
            inner,
            snapshots: self.snapshots.clone(),
        }
    }
}

#[derive(Clone)]
pub struct AccessLogService<S> {
    inner: S,
    snapshots: std::sync::Arc<SnapshotStore>,
}

impl<S> tonic::server::NamedService for AccessLogService<S>
where
    S: tonic::server::NamedService,
{
    const NAME: &'static str = S::NAME;
}

#[pin_project]
pub struct AccessLogFuture<F> {
    #[pin]
    inner: F,
    rpc: String,
    peer: String,
    requestor: String,
    start: Instant,
}

impl<S, ReqBody> Service<Request<ReqBody>> for AccessLogService<S>
where
    S: Service<Request<ReqBody>, Response = Response<BoxBody>> + Clone + Send + 'static,
    S::Future: Send + 'static,
    ReqBody: Send + 'static,
{
    type Response = Response<BoxBody>;
    type Error = S::Error;
    type Future = AccessLogFuture<S::Future>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: Request<ReqBody>) -> Self::Future {
        let rpc = rpc_path(&req);
        let peer = peer_addr(&req);
        let meta = MetadataMap::from_headers(req.headers().clone());
        let requestor = requestor_label(
            &self.snapshots,
            &meta,
            req.extensions(),
            peer_certs_present(&req),
        );
        let start = Instant::now();
        AccessLogFuture {
            inner: self.inner.call(req),
            rpc,
            peer,
            requestor,
            start,
        }
    }
}

impl<F, E> Future for AccessLogFuture<F>
where
    F: Future<Output = Result<Response<BoxBody>, E>>,
{
    type Output = Result<Response<BoxBody>, E>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.project();
        match this.inner.poll(cx) {
            Poll::Ready(Ok(res)) => {
                let code = grpc_code_from_response(&res);
                let latency_ms = this.start.elapsed().as_millis() as u64;
                log_control_rpc(this.rpc, this.peer, this.requestor, code, latency_ms);
                Poll::Ready(Ok(res))
            }
            Poll::Ready(Err(e)) => {
                let latency_ms = this.start.elapsed().as_millis() as u64;
                log_control_rpc(
                    this.rpc,
                    this.peer,
                    this.requestor,
                    Code::Unknown,
                    latency_ms,
                );
                Poll::Ready(Err(e))
            }
            Poll::Pending => Poll::Pending,
        }
    }
}

fn log_control_rpc(rpc: &str, peer: &str, requestor: &str, code: Code, latency_ms: u64) {
    tracing::info!(
        rpc,
        peer,
        requestor,
        grpc_code = ?code,
        latency_ms,
        "control rpc"
    );
}

fn rpc_path<B>(req: &Request<B>) -> String {
    if let Some(method) = req.extensions().get::<GrpcMethod<'_>>() {
        return format!("{}/{}", method.service(), method.method());
    }
    req.uri().path().to_string()
}

fn peer_addr<B>(req: &Request<B>) -> String {
    remote_socket_addr(req)
        .map(|a| a.to_string())
        .unwrap_or_else(|| "unknown".into())
}

fn remote_socket_addr<B>(req: &Request<B>) -> Option<std::net::SocketAddr> {
    req.extensions()
        .get::<TcpConnectInfo>()
        .and_then(|info| info.remote_addr())
        .or_else(|| {
            req.extensions()
                .get::<TlsConnectInfo<TcpConnectInfo>>()
                .and_then(|info| info.get_ref().remote_addr())
        })
}

fn peer_certs_present<B>(req: &Request<B>) -> bool {
    req.extensions()
        .get::<TlsConnectInfo<TcpConnectInfo>>()
        .and_then(|info| info.peer_certs())
        .is_some()
}

fn grpc_code_from_response(res: &Response<BoxBody>) -> Code {
    res.headers()
        .get("grpc-status")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.parse::<u32>().ok())
        .map(|raw| Code::from_i32(raw as i32))
        .unwrap_or(Code::Ok)
}

/// Map interceptor auth failure to a response and log line (auth runs inside inner service).
pub fn log_interceptor_denial(
    snapshots: &SnapshotStore,
    meta: &MetadataMap,
    extensions: &http::Extensions,
    status: &Status,
) {
    let rpc = extensions
        .get::<GrpcMethod<'_>>()
        .map(|m| format!("{}/{}", m.service(), m.method()))
        .unwrap_or_else(|| "unknown".into());
    let peer = extensions
        .get::<TcpConnectInfo>()
        .and_then(|i| i.remote_addr())
        .map(|a| a.to_string())
        .unwrap_or_else(|| "unknown".into());
    let requestor = requestor_label(
        snapshots,
        meta,
        extensions,
        peer_certs_present_extensions(extensions),
    );
    tracing::info!(
        rpc,
        peer,
        requestor,
        grpc_code = ?status.code(),
        latency_ms = 0_u64,
        "control rpc"
    );
}

fn peer_certs_present_extensions(extensions: &http::Extensions) -> bool {
    extensions
        .get::<TlsConnectInfo<TcpConnectInfo>>()
        .and_then(|info| info.peer_certs())
        .is_some()
}
