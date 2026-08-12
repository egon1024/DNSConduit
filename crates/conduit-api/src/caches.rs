//! Cache config primitive RPCs (`ConduitCaches`).

use crate::access_log::log_control_outcome;
use conduit_core::configurator::{ConfigPrimitive, ConfiguratorHandle};
use conduit_core::SnapshotStore;
use conduit_proto::config::{
    CacheInstance as RuntimeCacheInstance, CacheNegativeConfig as RuntimeNegativeConfig,
    CacheOnHitConfig as RuntimeOnHitConfig, CacheTruncatedUdpConfig as RuntimeTruncatedUdpConfig,
};
use conduit_proto::control::conduit_caches_server::ConduitCaches;
use conduit_proto::control::{
    ApplyConfigResponse, CacheInstance as ControlCacheInstance,
    CacheNegativeConfig as ControlNegativeConfig, CacheOnHitConfig as ControlOnHitConfig,
    CacheSummary, CacheTruncatedUdpConfig as ControlTruncatedUdpConfig, ConfigApplyStatusNote,
    GetCacheRequest, GetCacheResponse, ListCachesRequest, ListCachesResponse,
    SetCacheLmdbHotRequest, SetCacheMaxEntriesRequest, SetCachePolicyHotRequest,
};
use prost::Message;
use std::sync::Arc;
use tonic::{Request, Response, Status};

#[derive(Clone)]
pub struct CachesService {
    pub snapshots: Arc<SnapshotStore>,
    pub configurator: ConfiguratorHandle,
}

fn runtime_cache_to_control(cache: RuntimeCacheInstance) -> ControlCacheInstance {
    let bytes = cache.encode_to_vec();
    ControlCacheInstance::decode(bytes.as_slice())
        .expect("config and control CacheInstance are compatible")
}

fn control_negative_to_runtime(neg: ControlNegativeConfig) -> RuntimeNegativeConfig {
    let bytes = neg.encode_to_vec();
    RuntimeNegativeConfig::decode(bytes.as_slice())
        .expect("config and control CacheNegativeConfig are compatible")
}

fn control_on_hit_to_runtime(on_hit: ControlOnHitConfig) -> RuntimeOnHitConfig {
    let bytes = on_hit.encode_to_vec();
    RuntimeOnHitConfig::decode(bytes.as_slice())
        .expect("config and control CacheOnHitConfig are compatible")
}

fn control_truncated_udp_to_runtime(
    truncated: ControlTruncatedUdpConfig,
) -> RuntimeTruncatedUdpConfig {
    let bytes = truncated.encode_to_vec();
    RuntimeTruncatedUdpConfig::decode(bytes.as_slice())
        .expect("config and control CacheTruncatedUdpConfig are compatible")
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
impl ConduitCaches for CachesService {
    async fn list_caches(
        &self,
        _: Request<ListCachesRequest>,
    ) -> Result<Response<ListCachesResponse>, Status> {
        let snap = self.snapshots.load();
        let caches = snap
            .config
            .caches
            .iter()
            .map(|c| CacheSummary {
                name: c.name.clone(),
                r#type: c.r#type.clone(),
                max_entries: c.max_entries,
            })
            .collect();
        Ok(Response::new(ListCachesResponse { caches }))
    }

    async fn get_cache(
        &self,
        request: Request<GetCacheRequest>,
    ) -> Result<Response<GetCacheResponse>, Status> {
        let name = request.into_inner().name;
        if name.is_empty() {
            return Err(Status::invalid_argument("cache name is required"));
        }
        let snap = self.snapshots.load();
        let cache = snap
            .config
            .caches
            .iter()
            .find(|c| c.name == name)
            .cloned()
            .ok_or_else(|| Status::not_found(format!("unknown cache '{name}'")))?;
        Ok(Response::new(GetCacheResponse {
            cache: Some(runtime_cache_to_control(cache)),
        }))
    }

    async fn set_cache_max_entries(
        &self,
        request: Request<SetCacheMaxEntriesRequest>,
    ) -> Result<Response<ApplyConfigResponse>, Status> {
        let req = request.into_inner();
        if req.name.is_empty() {
            return Err(Status::invalid_argument("cache name is required"));
        }
        let result = self
            .configurator
            .apply_primitive(
                ConfigPrimitive::SetCacheMaxEntries {
                    name: req.name,
                    max_entries: req.max_entries,
                },
                None,
            )
            .await;
        log_control_outcome("SetCacheMaxEntries", result.ok, &result.errors);
        Ok(Response::new(apply_result_to_response(result)))
    }

    async fn set_cache_lmdb_hot(
        &self,
        request: Request<SetCacheLmdbHotRequest>,
    ) -> Result<Response<ApplyConfigResponse>, Status> {
        let req = request.into_inner();
        if req.name.is_empty() {
            return Err(Status::invalid_argument("cache name is required"));
        }
        if req.when_full.is_none()
            && req.sample_size.is_none()
            && req.sync.is_none()
            && req.sync_interval.is_none()
            && req.map_size_bytes.is_none()
        {
            return Err(Status::invalid_argument(
                "at least one LMDB hot field is required",
            ));
        }
        let result = self
            .configurator
            .apply_primitive(
                ConfigPrimitive::SetCacheLmdbHot {
                    name: req.name,
                    when_full: req.when_full,
                    sample_size: req.sample_size,
                    sync: req.sync,
                    sync_interval: req.sync_interval,
                    map_size_bytes: req.map_size_bytes,
                },
                None,
            )
            .await;
        log_control_outcome("SetCacheLmdbHot", result.ok, &result.errors);
        Ok(Response::new(apply_result_to_response(result)))
    }

    async fn set_cache_policy_hot(
        &self,
        request: Request<SetCachePolicyHotRequest>,
    ) -> Result<Response<ApplyConfigResponse>, Status> {
        let req = request.into_inner();
        if req.name.is_empty() {
            return Err(Status::invalid_argument("cache name is required"));
        }
        if req.negative_cache.is_none()
            && req.on_hit.is_none()
            && req.truncated_udp.is_none()
            && req.rotate_rrset_on_serve.is_none()
        {
            return Err(Status::invalid_argument(
                "at least one policy field is required",
            ));
        }
        let result = self
            .configurator
            .apply_primitive(
                ConfigPrimitive::SetCachePolicyHot {
                    name: req.name,
                    negative_cache: req.negative_cache.map(control_negative_to_runtime),
                    on_hit: req.on_hit.map(control_on_hit_to_runtime),
                    truncated_udp: req.truncated_udp.map(control_truncated_udp_to_runtime),
                    rotate_rrset_on_serve: req.rotate_rrset_on_serve,
                },
                None,
            )
            .await;
        log_control_outcome("SetCachePolicyHot", result.ok, &result.errors);
        Ok(Response::new(apply_result_to_response(result)))
    }
}
