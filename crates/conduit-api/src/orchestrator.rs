//! Orchestrator config primitive RPCs (`ConduitOrchestrator`).

use crate::access_log::log_control_outcome;
use conduit_core::configurator::{ConfigPrimitive, ConfiguratorHandle};
use conduit_core::SnapshotStore;
use conduit_proto::config::OrchestratorConfig as RuntimeOrchestrator;
use conduit_proto::control::conduit_orchestrator_server::ConduitOrchestrator;
use conduit_proto::control::{
    ApplyConfigResponse, ConfigApplyStatusNote, GetOrchestratorRequest, GetOrchestratorResponse,
    OrchestratorConfig as ControlOrchestrator, SetOrchestratorLimitsRequest,
};
use prost::Message;
use std::sync::Arc;
use tonic::{Request, Response, Status};

#[derive(Clone)]
pub struct OrchestratorService {
    pub snapshots: Arc<SnapshotStore>,
    pub configurator: ConfiguratorHandle,
}

fn runtime_orchestrator_to_control(orch: RuntimeOrchestrator) -> ControlOrchestrator {
    let bytes = orch.encode_to_vec();
    ControlOrchestrator::decode(bytes.as_slice())
        .expect("config and control OrchestratorConfig are compatible")
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
impl ConduitOrchestrator for OrchestratorService {
    async fn get_orchestrator(
        &self,
        _: Request<GetOrchestratorRequest>,
    ) -> Result<Response<GetOrchestratorResponse>, Status> {
        let snap = self.snapshots.load();
        let orchestrator = snap.config.orchestrator.clone().unwrap_or_default();
        Ok(Response::new(GetOrchestratorResponse {
            orchestrator: Some(runtime_orchestrator_to_control(orchestrator)),
        }))
    }

    async fn set_orchestrator_limits(
        &self,
        request: Request<SetOrchestratorLimitsRequest>,
    ) -> Result<Response<ApplyConfigResponse>, Status> {
        let req = request.into_inner();
        if req.max_attempts.is_none() && req.max_txn_duration_ms.is_none() {
            return Err(Status::invalid_argument(
                "at least one of max_attempts or max_txn_duration_ms is required",
            ));
        }
        let result = self
            .configurator
            .apply_primitive(
                ConfigPrimitive::SetOrchestratorLimits {
                    max_attempts: req.max_attempts,
                    max_txn_duration_ms: req.max_txn_duration_ms,
                },
                None,
            )
            .await;
        log_control_outcome("SetOrchestratorLimits", result.ok, &result.errors);
        Ok(Response::new(apply_result_to_response(result)))
    }
}
