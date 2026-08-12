//! Events config primitive RPCs (`ConduitEvents`).

use crate::access_log::log_control_outcome;
use conduit_core::configurator::{ConfigPrimitive, ConfiguratorHandle};
use conduit_core::SnapshotStore;
use conduit_proto::config::{
    EventSink as RuntimeEventSink, EventSinkFilters as RuntimeFilters,
    EventsConfig as RuntimeEventsConfig,
};
use conduit_proto::control::conduit_events_server::ConduitEvents;
use conduit_proto::control::{
    ApplyConfigResponse, ConfigApplyStatusNote, EventSink as ControlEventSink,
    EventSinkFilters as ControlFilters, EventsConfig as ControlEventsConfig, GetEventSinkRequest,
    GetEventSinkResponse, GetEventsRequest, GetEventsResponse, SetEventSinkEmitRequest,
    SetEventSinkFiltersRequest,
};
use prost::Message;
use std::sync::Arc;
use tonic::{Request, Response, Status};

#[derive(Clone)]
pub struct EventsService {
    pub snapshots: Arc<SnapshotStore>,
    pub configurator: ConfiguratorHandle,
}

fn runtime_events_to_control(events: RuntimeEventsConfig) -> ControlEventsConfig {
    let bytes = events.encode_to_vec();
    ControlEventsConfig::decode(bytes.as_slice())
        .expect("config and control EventsConfig are compatible")
}

fn runtime_sink_to_control(sink: RuntimeEventSink) -> ControlEventSink {
    let bytes = sink.encode_to_vec();
    ControlEventSink::decode(bytes.as_slice()).expect("config and control EventSink are compatible")
}

fn control_filters_to_runtime(filters: ControlFilters) -> RuntimeFilters {
    let bytes = filters.encode_to_vec();
    RuntimeFilters::decode(bytes.as_slice())
        .expect("config and control EventSinkFilters are compatible")
}

fn event_sink_name(sink: &RuntimeEventSink) -> &str {
    if let Some(n) = sink.name.as_deref().filter(|n| !n.is_empty()) {
        n
    } else if !sink.export_id.is_empty() {
        sink.export_id.as_str()
    } else {
        ""
    }
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
impl ConduitEvents for EventsService {
    async fn get_events(
        &self,
        _: Request<GetEventsRequest>,
    ) -> Result<Response<GetEventsResponse>, Status> {
        let snap = self.snapshots.load();
        let events = snap.config.events.clone().unwrap_or_default();
        Ok(Response::new(GetEventsResponse {
            events: Some(runtime_events_to_control(events)),
        }))
    }

    async fn get_event_sink(
        &self,
        request: Request<GetEventSinkRequest>,
    ) -> Result<Response<GetEventSinkResponse>, Status> {
        let name = request.into_inner().name;
        if name.is_empty() {
            return Err(Status::invalid_argument("event sink name is required"));
        }
        let snap = self.snapshots.load();
        let events = snap
            .config
            .events
            .as_ref()
            .ok_or_else(|| Status::not_found("no events section in effective config"))?;
        let sink = events
            .sinks
            .iter()
            .find(|s| event_sink_name(s) == name)
            .cloned()
            .ok_or_else(|| Status::not_found(format!("unknown event sink '{name}'")))?;
        Ok(Response::new(GetEventSinkResponse {
            sink: Some(runtime_sink_to_control(sink)),
        }))
    }

    async fn set_event_sink_filters(
        &self,
        request: Request<SetEventSinkFiltersRequest>,
    ) -> Result<Response<ApplyConfigResponse>, Status> {
        let req = request.into_inner();
        if req.name.is_empty() {
            return Err(Status::invalid_argument("event sink name is required"));
        }
        let filters = req
            .filters
            .ok_or_else(|| Status::invalid_argument("filters is required"))?;
        let runtime_filters = control_filters_to_runtime(filters);
        let result = self
            .configurator
            .apply_primitive(
                ConfigPrimitive::SetEventSinkFilters {
                    name: req.name,
                    filters: runtime_filters,
                },
                None,
            )
            .await;
        log_control_outcome("SetEventSinkFilters", result.ok, &result.errors);
        Ok(Response::new(apply_result_to_response(result)))
    }

    async fn set_event_sink_emit(
        &self,
        request: Request<SetEventSinkEmitRequest>,
    ) -> Result<Response<ApplyConfigResponse>, Status> {
        let req = request.into_inner();
        if req.name.is_empty() {
            return Err(Status::invalid_argument("event sink name is required"));
        }
        let result = self
            .configurator
            .apply_primitive(
                ConfigPrimitive::SetEventSinkEmit {
                    name: req.name,
                    emit: req.emit,
                    extra_fields: req.extra_fields,
                    extra_tags: req.extra_tags,
                    extra_fields_set: req.extra_fields_set,
                    extra_tags_set: req.extra_tags_set,
                },
                None,
            )
            .await;
        log_control_outcome("SetEventSinkEmit", result.ok, &result.errors);
        Ok(Response::new(apply_result_to_response(result)))
    }
}
