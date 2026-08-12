//! Data source config primitive RPCs (`ConduitDataSources`).

use crate::access_log::log_control_outcome;
use conduit_core::configurator::{ConfigPrimitive, ConfiguratorHandle};
use conduit_core::SnapshotStore;
use conduit_proto::config::{DataSource as RuntimeDataSource, DataSourceLimits as RuntimeLimits};
use conduit_proto::control::conduit_data_sources_server::ConduitDataSources;
use conduit_proto::control::{
    ApplyConfigResponse, ConfigApplyStatusNote, DataSource as ControlDataSource,
    DataSourceLimits as ControlLimits, DataSourceSummary, GetDataSourceLimitsRequest,
    GetDataSourceLimitsResponse, GetDataSourceRequest, GetDataSourceResponse,
    ListDataSourcesRequest, ListDataSourcesResponse, RemoveDataSourceRequest,
    SetDataSourceLimitsRequest, UpsertDataSourceRequest,
};
use prost::Message;
use std::sync::Arc;
use tonic::{Request, Response, Status};

#[derive(Clone)]
pub struct DataSourcesService {
    pub snapshots: Arc<SnapshotStore>,
    pub configurator: ConfiguratorHandle,
}

fn runtime_source_to_control(src: RuntimeDataSource) -> ControlDataSource {
    let bytes = src.encode_to_vec();
    ControlDataSource::decode(bytes.as_slice())
        .expect("config and control DataSource are compatible")
}

fn control_source_to_runtime(src: ControlDataSource) -> RuntimeDataSource {
    let bytes = src.encode_to_vec();
    RuntimeDataSource::decode(bytes.as_slice())
        .expect("config and control DataSource are compatible")
}

fn runtime_limits_to_control(limits: RuntimeLimits) -> ControlLimits {
    let bytes = limits.encode_to_vec();
    ControlLimits::decode(bytes.as_slice())
        .expect("config and control DataSourceLimits are compatible")
}

fn control_limits_to_runtime(limits: ControlLimits) -> RuntimeLimits {
    let bytes = limits.encode_to_vec();
    RuntimeLimits::decode(bytes.as_slice())
        .expect("config and control DataSourceLimits are compatible")
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
impl ConduitDataSources for DataSourcesService {
    async fn list_data_sources(
        &self,
        _: Request<ListDataSourcesRequest>,
    ) -> Result<Response<ListDataSourcesResponse>, Status> {
        let snap = self.snapshots.load();
        let sources = snap
            .config
            .data_sources
            .iter()
            .map(|s| DataSourceSummary {
                name: s.name.clone(),
                r#type: s.r#type.clone(),
                path: s.path.clone(),
            })
            .collect();
        Ok(Response::new(ListDataSourcesResponse { sources }))
    }

    async fn get_data_source(
        &self,
        request: Request<GetDataSourceRequest>,
    ) -> Result<Response<GetDataSourceResponse>, Status> {
        let name = request.into_inner().name;
        if name.is_empty() {
            return Err(Status::invalid_argument("data source name is required"));
        }
        let snap = self.snapshots.load();
        let source = snap
            .config
            .data_sources
            .iter()
            .find(|s| s.name == name)
            .cloned()
            .ok_or_else(|| Status::not_found(format!("unknown data source '{name}'")))?;
        Ok(Response::new(GetDataSourceResponse {
            source: Some(runtime_source_to_control(source)),
        }))
    }

    async fn upsert_data_source(
        &self,
        request: Request<UpsertDataSourceRequest>,
    ) -> Result<Response<ApplyConfigResponse>, Status> {
        let req = request.into_inner();
        let source = req
            .source
            .ok_or_else(|| Status::invalid_argument("source is required"))?;
        if source.name.is_empty() {
            return Err(Status::invalid_argument("data source name is required"));
        }
        let runtime_source = control_source_to_runtime(source);
        let result = self
            .configurator
            .apply_primitive(
                ConfigPrimitive::UpsertDataSource {
                    source: Box::new(runtime_source),
                },
                None,
            )
            .await;
        log_control_outcome("UpsertDataSource", result.ok, &result.errors);
        Ok(Response::new(apply_result_to_response(result)))
    }

    async fn remove_data_source(
        &self,
        request: Request<RemoveDataSourceRequest>,
    ) -> Result<Response<ApplyConfigResponse>, Status> {
        let name = request.into_inner().name;
        if name.is_empty() {
            return Err(Status::invalid_argument("data source name is required"));
        }
        let result = self
            .configurator
            .apply_primitive(ConfigPrimitive::RemoveDataSource { name }, None)
            .await;
        log_control_outcome("RemoveDataSource", result.ok, &result.errors);
        Ok(Response::new(apply_result_to_response(result)))
    }

    async fn get_data_source_limits(
        &self,
        _: Request<GetDataSourceLimitsRequest>,
    ) -> Result<Response<GetDataSourceLimitsResponse>, Status> {
        let snap = self.snapshots.load();
        let limits = snap.config.data_source_limits.unwrap_or_default();
        Ok(Response::new(GetDataSourceLimitsResponse {
            limits: Some(runtime_limits_to_control(limits)),
        }))
    }

    async fn set_data_source_limits(
        &self,
        request: Request<SetDataSourceLimitsRequest>,
    ) -> Result<Response<ApplyConfigResponse>, Status> {
        let req = request.into_inner();
        let limits = req
            .limits
            .ok_or_else(|| Status::invalid_argument("limits is required"))?;
        let runtime_limits = control_limits_to_runtime(limits);
        let result = self
            .configurator
            .apply_primitive(
                ConfigPrimitive::SetDataSourceLimits {
                    limits: runtime_limits,
                },
                None,
            )
            .await;
        log_control_outcome("SetDataSourceLimits", result.ok, &result.errors);
        Ok(Response::new(apply_result_to_response(result)))
    }
}
