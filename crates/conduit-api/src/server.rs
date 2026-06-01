//! gRPC control plane service (spec §8).

use crate::access_log::{AccessLogLayer, AccessLogService};
use crate::auth::ControlInterceptor;
use crate::tls::server_tls_config;
use conduit_config::{export_yaml, validate, EffectiveConfig};
use conduit_core::configurator::{ConfiguratorHandle, ProposalSource};
use conduit_core::snapshot::SnapshotStore;
use conduit_metrics::TracingHub;
use conduit_proto::config::Config as RuntimeConfig;
use conduit_proto::control::conduit_control_server::{ConduitControl, ConduitControlServer};
use conduit_proto::control::Config as ControlConfig;
use conduit_proto::control::{
    ApplyConfigRequest, ApplyConfigResponse, ExportConfigRequest, ExportConfigResponse,
    GetConfigRequest, GetConfigResponse, GetTraceRequest, GetTraceResponse, HealthRequest,
    HealthResponse, ReloadFromFileRequest, ReloadFromFileResponse, TraceEvent as TraceEventProto,
    ValidateConfigRequest, ValidateConfigResponse,
};
use prost::Message;
use std::net::SocketAddr;
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
        Ok(Response::new(ValidateConfigResponse {
            ok: v.ok,
            errors: v.errors,
        }))
    }

    async fn apply_config(
        &self,
        request: Request<ApplyConfigRequest>,
    ) -> Result<Response<ApplyConfigResponse>, Status> {
        let overlay = request
            .into_inner()
            .overlay
            .ok_or_else(|| Status::invalid_argument("missing overlay"))?;
        let result = self
            .configurator
            .apply_overlay(control_to_runtime(overlay), None)
            .await;
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
        let trace_id = request.into_inner().trace_id;
        let events = self.tracing.store.get(&trace_id);
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

fn build_server(
    snapshots: Arc<SnapshotStore>,
    effective: Arc<Mutex<EffectiveConfig>>,
    configurator: ConfiguratorHandle,
    tracing: Arc<TracingHub>,
) -> InterceptedControlService {
    let inner = ConduitControlServer::with_interceptor(
        ControlService {
            snapshots: snapshots.clone(),
            effective,
            configurator,
            tracing,
        },
        ControlInterceptor::new(snapshots.clone()),
    );
    AccessLogLayer::new(snapshots).layer(inner)
}

fn apply_tls(
    builder: tonic::transport::server::Server,
    snapshots: &SnapshotStore,
) -> anyhow::Result<tonic::transport::server::Server> {
    let snap = snapshots.load();
    let tls = snap.config.control.as_ref().and_then(|c| c.tls.as_ref());
    if let Some(tls) = tls {
        if tls.cert_path.is_empty() || tls.key_path.is_empty() {
            anyhow::bail!("control.tls requires cert_path and key_path");
        }
        let tls_config = server_tls_config(tls)?;
        Ok(builder.tls_config(tls_config)?)
    } else {
        Ok(builder)
    }
}

/// Run the gRPC control server until shutdown.
pub async fn serve(
    addr: SocketAddr,
    snapshots: Arc<SnapshotStore>,
    effective: Arc<Mutex<EffectiveConfig>>,
    configurator: ConfiguratorHandle,
    tracing: Arc<TracingHub>,
) -> anyhow::Result<()> {
    let reflection_enabled = snapshots
        .load()
        .config
        .control
        .as_ref()
        .map(|c| c.reflection_enabled)
        .unwrap_or(false);
    let service = build_server(snapshots.clone(), effective, configurator, tracing);
    let mut builder = apply_tls(Server::builder(), &snapshots)?;
    if reflection_enabled {
        let reflection = tonic_reflection::server::Builder::configure()
            .register_encoded_file_descriptor_set(conduit_proto::FILE_DESCRIPTOR_SET)
            .build_v1alpha()?;
        builder
            .add_service(reflection)
            .add_service(service)
            .serve(addr)
            .await?;
    } else {
        builder.add_service(service).serve(addr).await?;
    }
    Ok(())
}

/// Bind `addr` (use port `0` for tests), return the resolved local address, and run the server on that listener.
pub async fn serve_on_listener(
    addr: SocketAddr,
    snapshots: Arc<SnapshotStore>,
    effective: Arc<Mutex<EffectiveConfig>>,
    configurator: ConfiguratorHandle,
    tracing: Arc<TracingHub>,
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
    let service = build_server(snapshots.clone(), effective, configurator, tracing);
    let incoming = tokio_stream::wrappers::TcpListenerStream::new(listener);
    tokio::spawn(async move {
        let result = if reflection_enabled {
            match tonic_reflection::server::Builder::configure()
                .register_encoded_file_descriptor_set(conduit_proto::FILE_DESCRIPTOR_SET)
                .build_v1alpha()
            {
                Ok(reflection) => match apply_tls(Server::builder(), &snapshots) {
                    Ok(mut builder) => {
                        builder
                            .add_service(reflection)
                            .add_service(service)
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
            match apply_tls(Server::builder(), &snapshots) {
                Ok(mut builder) => {
                    builder
                        .add_service(service)
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
