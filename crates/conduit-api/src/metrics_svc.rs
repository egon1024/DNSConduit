//! Metrics config primitive RPCs (`ConduitMetrics`).
//!
//! Module named `metrics_svc` to avoid colliding with `conduit_metrics` crate.

use crate::access_log::log_control_outcome;
use conduit_core::configurator::{ConfigPrimitive, ConfiguratorHandle};
use conduit_core::SnapshotStore;
use conduit_proto::config::MetricsConfig as RuntimeMetricsConfig;
use conduit_proto::control::conduit_metrics_server::ConduitMetrics;
use conduit_proto::control::{
    ApplyConfigResponse, ConfigApplyStatusNote, GetMetricsRequest, GetMetricsResponse,
    MetricsConfig as ControlMetricsConfig, PatchMetricsRequest,
};
use prost::Message;
use std::sync::Arc;
use tonic::{Request, Response, Status};

#[derive(Clone)]
pub struct MetricsSvcService {
    pub snapshots: Arc<SnapshotStore>,
    pub configurator: ConfiguratorHandle,
}

fn runtime_metrics_to_control(metrics: RuntimeMetricsConfig) -> ControlMetricsConfig {
    let bytes = metrics.encode_to_vec();
    ControlMetricsConfig::decode(bytes.as_slice())
        .expect("config and control MetricsConfig are compatible")
}

fn control_metrics_to_runtime(metrics: ControlMetricsConfig) -> RuntimeMetricsConfig {
    let bytes = metrics.encode_to_vec();
    RuntimeMetricsConfig::decode(bytes.as_slice())
        .expect("config and control MetricsConfig are compatible")
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
impl ConduitMetrics for MetricsSvcService {
    async fn get_metrics(
        &self,
        _: Request<GetMetricsRequest>,
    ) -> Result<Response<GetMetricsResponse>, Status> {
        let snap = self.snapshots.load();
        let metrics = snap.config.metrics.clone().unwrap_or_default();
        Ok(Response::new(GetMetricsResponse {
            metrics: Some(runtime_metrics_to_control(metrics)),
        }))
    }

    async fn patch_metrics(
        &self,
        request: Request<PatchMetricsRequest>,
    ) -> Result<Response<ApplyConfigResponse>, Status> {
        let req = request.into_inner();
        let metrics = req
            .metrics
            .ok_or_else(|| Status::invalid_argument("metrics is required"))?;
        let runtime_metrics = control_metrics_to_runtime(metrics);
        let result = self
            .configurator
            .apply_primitive(
                ConfigPrimitive::PatchMetrics {
                    metrics: Box::new(runtime_metrics),
                },
                None,
            )
            .await;
        log_control_outcome("PatchMetrics", result.ok, &result.errors);
        Ok(Response::new(apply_result_to_response(result)))
    }
}
