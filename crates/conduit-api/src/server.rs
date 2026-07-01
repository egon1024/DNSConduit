//! gRPC control plane service (spec §8).

use crate::access_log::{log_control_outcome, AccessLogLayer, AccessLogService};
use crate::auth::ControlInterceptor;
use crate::health::BackendHealthService;
use crate::tls::server_tls_config;
use conduit_config::{export_yaml, validate, EffectiveConfig};
use conduit_core::configurator::{ConfiguratorHandle, OverlayApplyMode, ProposalSource};
use conduit_core::snapshot::SnapshotStore;
use conduit_metrics::TracingHub;
use conduit_proto::config::Config as RuntimeConfig;
use conduit_proto::control::backend_health_server::BackendHealthServer;
use conduit_proto::control::conduit_control_server::{ConduitControl, ConduitControlServer};
use conduit_proto::control::Config as ControlConfig;
use conduit_proto::control::OverlayApplyMode as ProtoOverlayApplyMode;
use conduit_proto::control::{
    ApplyConfigRequest, ApplyConfigResponse, ExportConfigRequest, ExportConfigResponse,
    GetConfigRequest, GetConfigResponse, GetTraceRequest, GetTraceResponse, HealthRequest,
    HealthResponse, ReloadFromFileRequest, ReloadFromFileResponse, TraceEvent as TraceEventProto,
    ValidateConfigRequest, ValidateConfigResponse,
};
use prost::Message;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use tonic::transport::Server;
use tonic::{Request, Response, Status};
use tower::Layer;

#[derive(Clone)]
pub struct ControlService {
    pub snapshots: Arc<SnapshotStore>,
    pub effective: Arc<Mutex<EffectiveConfig>>,
    pub configurator: ConfiguratorHandle,
    pub tracing: Arc<TracingHub>,
}

/// Map `config` module types to `control` module types (same protobuf schema, separate Rust paths).
fn runtime_to_control(cfg: RuntimeConfig) -> ControlConfig {
    let bytes = cfg.encode_to_vec();
    ControlConfig::decode(bytes.as_slice()).expect("config and control Config are compatible")
}

fn control_to_runtime(cfg: ControlConfig) -> RuntimeConfig {
    let bytes = cfg.encode_to_vec();
    RuntimeConfig::decode(bytes.as_slice()).expect("config and control Config are compatible")
}

fn proto_overlay_mode(mode: i32) -> OverlayApplyMode {
    match ProtoOverlayApplyMode::try_from(mode).unwrap_or(ProtoOverlayApplyMode::Unspecified) {
        ProtoOverlayApplyMode::Replace => OverlayApplyMode::Replace,
        ProtoOverlayApplyMode::Clear => OverlayApplyMode::Clear,
        ProtoOverlayApplyMode::Merge | ProtoOverlayApplyMode::Unspecified => {
            OverlayApplyMode::Merge
        }
    }
}

#[tonic::async_trait]
impl ConduitControl for ControlService {
    async fn get_config(
        &self,
        _: Request<GetConfigRequest>,
    ) -> Result<Response<GetConfigResponse>, Status> {
        let snap = self.snapshots.load();
        Ok(Response::new(GetConfigResponse {
            effective: Some(runtime_to_control(snap.config.clone())),
        }))
    }

    async fn validate_config(
        &self,
        request: Request<ValidateConfigRequest>,
    ) -> Result<Response<ValidateConfigResponse>, Status> {
        let cfg = request
            .into_inner()
            .config
            .ok_or_else(|| Status::invalid_argument("missing config"))?;
        let v = validate(&control_to_runtime(cfg));
        log_control_outcome("ValidateConfig", v.ok, &v.errors);
        Ok(Response::new(ValidateConfigResponse {
            ok: v.ok,
            errors: v.errors,
        }))
    }

    async fn apply_config(
        &self,
        request: Request<ApplyConfigRequest>,
    ) -> Result<Response<ApplyConfigResponse>, Status> {
        let req = request.into_inner();
        let mode = proto_overlay_mode(req.mode);
        let (overlay, mode) = match mode {
            OverlayApplyMode::Clear => (None, mode),
            OverlayApplyMode::Merge | OverlayApplyMode::Replace => {
                let overlay = req
                    .overlay
                    .ok_or_else(|| Status::invalid_argument("missing overlay"))?;
                (Some(control_to_runtime(overlay)), mode)
            }
        };
        let result = self.configurator.apply_overlay(overlay, mode, None).await;
        log_control_outcome("ApplyConfig", result.ok, &result.errors);
        Ok(Response::new(ApplyConfigResponse {
            ok: result.ok,
            errors: result.errors,
        }))
    }

    async fn export_config(
        &self,
        request: Request<ExportConfigRequest>,
    ) -> Result<Response<ExportConfigResponse>, Status> {
        let format = request.into_inner().format;
        if !format.is_empty() && format != "yaml" {
            return Err(Status::invalid_argument(
                "only format \"yaml\" is supported",
            ));
        }

        let effective = self
            .effective
            .lock()
            .map_err(|_| Status::internal("effective config lock poisoned"))?;
        let cfg = effective.effective();
        let body = export_yaml(&cfg).map_err(|e| Status::internal(e.to_string()))?;
        Ok(Response::new(ExportConfigResponse { body }))
    }

    async fn reload_from_file(
        &self,
        _: Request<ReloadFromFileRequest>,
    ) -> Result<Response<ReloadFromFileResponse>, Status> {
        let result = self
            .configurator
            .reload_from_file(ProposalSource::File)
            .await;
        log_control_outcome("ReloadFromFile", result.ok, &result.errors);
        Ok(Response::new(ReloadFromFileResponse {
            ok: result.ok,
            errors: result.errors,
        }))
    }

    async fn health(&self, _: Request<HealthRequest>) -> Result<Response<HealthResponse>, Status> {
        Ok(Response::new(HealthResponse {
            status: "serving".into(),
        }))
    }

    async fn get_trace(
        &self,
        request: Request<GetTraceRequest>,
    ) -> Result<Response<GetTraceResponse>, Status> {
        let txn_id = request.into_inner().txn_id;
        let events = self.tracing.store.get(&txn_id);
        Ok(Response::new(GetTraceResponse {
            found: events.is_some(),
            events: events
                .unwrap_or_default()
                .into_iter()
                .map(|e| TraceEventProto {
                    phase: e.phase,
                    elapsed_us: e.elapsed_us,
                    message: e.message,
                    pool: e.pool,
                    backend: e.backend,
                })
                .collect(),
        }))
    }
}

type InterceptedControlService = AccessLogService<
    tonic::service::interceptor::InterceptedService<
        ConduitControlServer<ControlService>,
        ControlInterceptor,
    >,
>;

type InterceptedBackendHealthService = AccessLogService<
    tonic::service::interceptor::InterceptedService<
        BackendHealthServer<BackendHealthService>,
        ControlInterceptor,
    >,
>;

struct ControlPlaneServices {
    control: InterceptedControlService,
    health: InterceptedBackendHealthService,
}

fn build_servers(
    snapshots: Arc<SnapshotStore>,
    effective: Arc<Mutex<EffectiveConfig>>,
    configurator: ConfiguratorHandle,
    tracing: Arc<TracingHub>,
) -> ControlPlaneServices {
    let interceptor = ControlInterceptor::new(snapshots.clone());
    let control_inner = ConduitControlServer::with_interceptor(
        ControlService {
            snapshots: snapshots.clone(),
            effective,
            configurator,
            tracing,
        },
        interceptor.clone(),
    );
    let health_inner = BackendHealthServer::with_interceptor(
        BackendHealthService {
            snapshots: snapshots.clone(),
        },
        interceptor,
    );
    let layer = AccessLogLayer::new(snapshots);
    ControlPlaneServices {
        control: layer.layer(control_inner),
        health: layer.layer(health_inner),
    }
}

fn apply_tls(
    builder: tonic::transport::server::Server,
    snapshots: &SnapshotStore,
    base_dir: Option<&Path>,
) -> anyhow::Result<tonic::transport::server::Server> {
    let snap = snapshots.load();
    let tls = snap.config.control.as_ref().and_then(|c| c.tls.as_ref());
    if let Some(tls) = tls {
        if tls.cert_path.is_empty() || tls.key_path.is_empty() {
            anyhow::bail!("control.tls requires cert_path and key_path");
        }
        let tls_config = server_tls_config(tls, base_dir)?;
        Ok(builder.tls_config(tls_config)?)
    } else {
        Ok(builder)
    }
}

/// Handle for a background control-plane server task.
pub struct ControlHandle {
    shutdown_tx: tokio::sync::oneshot::Sender<()>,
    join: tokio::task::JoinHandle<anyhow::Result<()>>,
}

impl ControlHandle {
    pub async fn shutdown(self) {
        let _ = self.shutdown_tx.send(());
        match self.join.await {
            Ok(Ok(())) => {}
            Ok(Err(e)) => tracing::warn!(error = %e, "control plane exited with error"),
            Err(e) if e.is_panic() => std::panic::resume_unwind(e.into_panic()),
            Err(e) => tracing::warn!(error = %e, "control plane task failed"),
        }
    }
}

async fn run_control_plane<S>(
    addr: SocketAddr,
    snapshots: Arc<SnapshotStore>,
    effective: Arc<Mutex<EffectiveConfig>>,
    configurator: ConfiguratorHandle,
    tracing: Arc<TracingHub>,
    config_base_dir: Option<PathBuf>,
    shutdown: S,
) -> anyhow::Result<()>
where
    S: std::future::Future<Output = ()> + Send + 'static,
{
    let reflection_enabled = snapshots
        .load()
        .config
        .control
        .as_ref()
        .is_some_and(|c| c.reflection_enabled);
    let services = build_servers(snapshots.clone(), effective, configurator, tracing);
    let listener = tokio::net::TcpListener::bind(addr).await?;
    let incoming = tokio_stream::wrappers::TcpListenerStream::new(listener);
    let base_dir = config_base_dir.as_deref();
    let mut builder = apply_tls(Server::builder(), &snapshots, base_dir)?;
    if reflection_enabled {
        let reflection = tonic_reflection::server::Builder::configure()
            .register_encoded_file_descriptor_set(conduit_proto::FILE_DESCRIPTOR_SET)
            .build_v1alpha()?;
        builder
            .add_service(reflection)
            .add_service(services.control)
            .add_service(services.health)
            .serve_with_incoming_shutdown(incoming, shutdown)
            .await?;
    } else {
        builder
            .add_service(services.control)
            .add_service(services.health)
            .serve_with_incoming_shutdown(incoming, shutdown)
            .await?;
    }
    Ok(())
}

/// Spawn the gRPC control server on a background task with graceful shutdown.
pub fn spawn_control_plane(
    addr: SocketAddr,
    snapshots: Arc<SnapshotStore>,
    effective: Arc<Mutex<EffectiveConfig>>,
    configurator: ConfiguratorHandle,
    tracing: Arc<TracingHub>,
    config_base_dir: Option<PathBuf>,
) -> anyhow::Result<ControlHandle> {
    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel();
    let join = tokio::spawn(run_control_plane(
        addr,
        snapshots,
        effective,
        configurator,
        tracing,
        config_base_dir,
        async move {
            let _ = shutdown_rx.await;
        },
    ));
    Ok(ControlHandle { shutdown_tx, join })
}

/// Run the gRPC control server until the task is cancelled (blocks the current task).
pub async fn serve(
    addr: SocketAddr,
    snapshots: Arc<SnapshotStore>,
    effective: Arc<Mutex<EffectiveConfig>>,
    configurator: ConfiguratorHandle,
    tracing: Arc<TracingHub>,
    config_base_dir: Option<PathBuf>,
) -> anyhow::Result<()> {
    run_control_plane(
        addr,
        snapshots,
        effective,
        configurator,
        tracing,
        config_base_dir,
        std::future::pending(),
    )
    .await
}

/// Bind `addr` (use port `0` for tests), return the resolved local address, and run the server on that listener.
pub async fn serve_on_listener(
    addr: SocketAddr,
    snapshots: Arc<SnapshotStore>,
    effective: Arc<Mutex<EffectiveConfig>>,
    configurator: ConfiguratorHandle,
    tracing: Arc<TracingHub>,
    config_base_dir: Option<PathBuf>,
) -> anyhow::Result<SocketAddr> {
    let reflection_enabled = snapshots
        .load()
        .config
        .control
        .as_ref()
        .map(|c| c.reflection_enabled)
        .unwrap_or(false);
    let listener = tokio::net::TcpListener::bind(addr).await?;
    let local_addr = listener.local_addr()?;
    let services = build_servers(snapshots.clone(), effective, configurator, tracing);
    let incoming = tokio_stream::wrappers::TcpListenerStream::new(listener);
    tokio::spawn(async move {
        let base_dir = config_base_dir.as_deref();
        let result = if reflection_enabled {
            match tonic_reflection::server::Builder::configure()
                .register_encoded_file_descriptor_set(conduit_proto::FILE_DESCRIPTOR_SET)
                .build_v1alpha()
            {
                Ok(reflection) => match apply_tls(Server::builder(), &snapshots, base_dir) {
                    Ok(mut builder) => {
                        builder
                            .add_service(reflection)
                            .add_service(services.control)
                            .add_service(services.health)
                            .serve_with_incoming(incoming)
                            .await
                    }
                    Err(e) => {
                        tracing::error!("control TLS config: {e}");
                        return;
                    }
                },
                Err(e) => {
                    tracing::error!("failed to build reflection service: {e}");
                    return;
                }
            }
        } else {
            match apply_tls(Server::builder(), &snapshots, base_dir) {
                Ok(mut builder) => {
                    builder
                        .add_service(services.control)
                        .add_service(services.health)
                        .serve_with_incoming(incoming)
                        .await
                }
                Err(e) => {
                    tracing::error!("control TLS config: {e}");
                    return;
                }
            }
        };
        if let Err(e) = result {
            tracing::error!("control server error: {e}");
        }
    });
    Ok(local_addr)
}
