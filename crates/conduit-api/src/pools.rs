//! Pool/backend config primitive RPCs (`ConduitPools`).

use crate::access_log::log_control_outcome;
use conduit_core::configurator::{ConfigPrimitive, ConfiguratorHandle};
use conduit_core::SnapshotStore;
use conduit_proto::config::{Backend as RuntimeBackend, Pool as RuntimePool};
use conduit_proto::control::conduit_pools_server::ConduitPools;
use conduit_proto::control::{
    AddBackendRequest, ApplyConfigResponse, Backend as ControlBackend, ConfigApplyStatusNote,
    GetPoolRequest, GetPoolResponse, ListPoolsRequest, ListPoolsResponse, Pool as ControlPool,
    PoolSummary, RemoveBackendRequest, SetBackendWeightRequest,
};
use prost::Message;
use std::sync::Arc;
use tonic::{Request, Response, Status};

#[derive(Clone)]
pub struct PoolsService {
    pub snapshots: Arc<SnapshotStore>,
    pub configurator: ConfiguratorHandle,
}

fn runtime_pool_to_control(pool: RuntimePool) -> ControlPool {
    let bytes = pool.encode_to_vec();
    ControlPool::decode(bytes.as_slice()).expect("config and control Pool are compatible")
}

fn control_backend_to_runtime(backend: ControlBackend) -> RuntimeBackend {
    let bytes = backend.encode_to_vec();
    RuntimeBackend::decode(bytes.as_slice()).expect("config and control Backend are compatible")
}

fn apply_result_to_response(result: conduit_core::ApplyResult) -> ApplyConfigResponse {
    ApplyConfigResponse {
        ok: result.ok,
        errors: result.errors,
        generation: result.generation,
        notes: result
            .notes
            .into_iter()
            .map(|n| ConfigApplyStatusNote {
                kind: n.kind,
                message: n.message,
            })
            .collect(),
    }
}

#[tonic::async_trait]
impl ConduitPools for PoolsService {
    async fn list_pools(
        &self,
        _: Request<ListPoolsRequest>,
    ) -> Result<Response<ListPoolsResponse>, Status> {
        let snap = self.snapshots.load();
        let pools = snap
            .config
            .pools
            .iter()
            .map(|p| PoolSummary {
                name: p.name.clone(),
                backend_count: p.backends.len() as u32,
            })
            .collect();
        Ok(Response::new(ListPoolsResponse { pools }))
    }

    async fn get_pool(
        &self,
        request: Request<GetPoolRequest>,
    ) -> Result<Response<GetPoolResponse>, Status> {
        let name = request.into_inner().name;
        if name.is_empty() {
            return Err(Status::invalid_argument("pool name is required"));
        }
        let snap = self.snapshots.load();
        let pool = snap
            .config
            .pools
            .iter()
            .find(|p| p.name == name)
            .cloned()
            .ok_or_else(|| Status::not_found(format!("unknown pool '{name}'")))?;
        Ok(Response::new(GetPoolResponse {
            pool: Some(runtime_pool_to_control(pool)),
        }))
    }

    async fn set_backend_weight(
        &self,
        request: Request<SetBackendWeightRequest>,
    ) -> Result<Response<ApplyConfigResponse>, Status> {
        let req = request.into_inner();
        if req.pool.is_empty() {
            return Err(Status::invalid_argument("pool is required"));
        }
        if req.backend.is_empty() {
            return Err(Status::invalid_argument("backend is required"));
        }
        let result = self
            .configurator
            .apply_primitive(
                ConfigPrimitive::SetBackendWeight {
                    pool: req.pool,
                    backend: req.backend,
                    weight: req.weight,
                },
                None,
            )
            .await;
        log_control_outcome("SetBackendWeight", result.ok, &result.errors);
        Ok(Response::new(apply_result_to_response(result)))
    }

    async fn add_backend(
        &self,
        request: Request<AddBackendRequest>,
    ) -> Result<Response<ApplyConfigResponse>, Status> {
        let req = request.into_inner();
        if req.pool.is_empty() {
            return Err(Status::invalid_argument("pool is required"));
        }
        let backend = req
            .backend
            .ok_or_else(|| Status::invalid_argument("backend is required"))?;
        let result = self
            .configurator
            .apply_primitive(
                ConfigPrimitive::AddBackend {
                    pool: req.pool,
                    backend: control_backend_to_runtime(backend),
                },
                None,
            )
            .await;
        log_control_outcome("AddBackend", result.ok, &result.errors);
        Ok(Response::new(apply_result_to_response(result)))
    }

    async fn remove_backend(
        &self,
        request: Request<RemoveBackendRequest>,
    ) -> Result<Response<ApplyConfigResponse>, Status> {
        let req = request.into_inner();
        if req.pool.is_empty() {
            return Err(Status::invalid_argument("pool is required"));
        }
        if req.backend.is_empty() {
            return Err(Status::invalid_argument("backend is required"));
        }
        let result = self
            .configurator
            .apply_primitive(
                ConfigPrimitive::RemoveBackend {
                    pool: req.pool,
                    backend: req.backend,
                },
                None,
            )
            .await;
        log_control_outcome("RemoveBackend", result.ok, &result.errors);
        Ok(Response::new(apply_result_to_response(result)))
    }
}
