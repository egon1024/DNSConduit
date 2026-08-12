//! Rhai config primitive RPCs (`ConduitRhai`).

use crate::access_log::log_control_outcome;
use conduit_core::configurator::{ConfigPrimitive, ConfiguratorHandle};
use conduit_core::SnapshotStore;
use conduit_proto::config::RhaiConfig as RuntimeRhaiConfig;
use conduit_proto::control::conduit_rhai_server::ConduitRhai;
use conduit_proto::control::{
    ApplyConfigResponse, ConfigApplyStatusNote, GetRhaiRequest, GetRhaiResponse,
    RhaiConfig as ControlRhaiConfig, SetRhaiLimitsRequest,
};
use prost::Message;
use std::sync::Arc;
use tonic::{Request, Response, Status};

#[derive(Clone)]
pub struct RhaiService {
    pub snapshots: Arc<SnapshotStore>,
    pub configurator: ConfiguratorHandle,
}

fn runtime_rhai_to_control(rhai: RuntimeRhaiConfig) -> ControlRhaiConfig {
    let bytes = rhai.encode_to_vec();
    ControlRhaiConfig::decode(bytes.as_slice())
        .expect("config and control RhaiConfig are compatible")
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
impl ConduitRhai for RhaiService {
    async fn get_rhai(
        &self,
        _: Request<GetRhaiRequest>,
    ) -> Result<Response<GetRhaiResponse>, Status> {
        let snap = self.snapshots.load();
        let rhai = snap.config.rhai.unwrap_or_default();
        Ok(Response::new(GetRhaiResponse {
            rhai: Some(runtime_rhai_to_control(rhai)),
        }))
    }

    async fn set_rhai_limits(
        &self,
        request: Request<SetRhaiLimitsRequest>,
    ) -> Result<Response<ApplyConfigResponse>, Status> {
        let req = request.into_inner();
        if req.max_operations.is_none()
            && req.max_call_depth.is_none()
            && req.hook_timeout_ms.is_none()
        {
            return Err(Status::invalid_argument(
                "at least one of max_operations, max_call_depth, or hook_timeout_ms is required",
            ));
        }
        let result = self
            .configurator
            .apply_primitive(
                ConfigPrimitive::SetRhaiLimits {
                    max_operations: req.max_operations,
                    max_call_depth: req.max_call_depth,
                    hook_timeout_ms: req.hook_timeout_ms,
                },
                None,
            )
            .await;
        log_control_outcome("SetRhaiLimits", result.ok, &result.errors);
        Ok(Response::new(apply_result_to_response(result)))
    }
}
