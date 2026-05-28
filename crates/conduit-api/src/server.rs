//! gRPC control plane service (spec §8).

use conduit_config::{export_yaml, validate, EffectiveConfig};
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

#[derive(Clone)]
pub struct ControlService {
    pub snapshots: Arc<SnapshotStore>,
    pub effective: Arc<Mutex<EffectiveConfig>>,
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
        _: Request<ApplyConfigRequest>,
    ) -> Result<Response<ApplyConfigResponse>, Status> {
        Err(Status::unimplemented("ApplyConfig is planned for Phase 5"))
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
        Err(Status::unimplemented(
            "ReloadFromFile is planned for Phase 5",
        ))
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

/// Run the gRPC control server until shutdown.
pub async fn serve(
    addr: SocketAddr,
    snapshots: Arc<SnapshotStore>,
    effective: Arc<Mutex<EffectiveConfig>>,
    tracing: Arc<TracingHub>,
) -> anyhow::Result<()> {
    let service = ConduitControlServer::new(ControlService {
        snapshots,
        effective,
        tracing,
    });
    Server::builder().add_service(service).serve(addr).await?;
    Ok(())
}

/// Bind `addr` (use port `0` for tests), return the resolved local address, and run the server on that listener.
pub async fn serve_on_listener(
    addr: SocketAddr,
    snapshots: Arc<SnapshotStore>,
    effective: Arc<Mutex<EffectiveConfig>>,
    tracing: Arc<TracingHub>,
) -> anyhow::Result<SocketAddr> {
    let listener = tokio::net::TcpListener::bind(addr).await?;
    let local_addr = listener.local_addr()?;
    let service = ConduitControlServer::new(ControlService {
        snapshots,
        effective,
        tracing,
    });
    let incoming = tokio_stream::wrappers::TcpListenerStream::new(listener);
    tokio::spawn(async move {
        if let Err(e) = Server::builder()
            .add_service(service)
            .serve_with_incoming(incoming)
            .await
        {
            tracing::error!("control server error: {e}");
        }
    });
    Ok(local_addr)
}
