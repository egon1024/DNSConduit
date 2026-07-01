//! Backend health operator RPCs (phase 1c §D8).

use conduit_config::health::CompiledHealth;
use conduit_core::health::{
    BackendHealthFilter, EffectiveScope, Health, HealthControlAction, HealthControlScope,
};
use conduit_core::SnapshotStore;
use conduit_proto::control::backend_health_server::BackendHealth;
use conduit_proto::control::{
    BackendHealthEntry, BackendHealthFilter as ProtoFilter, GetBackendHealthRequest,
    GetBackendHealthResponse, HealthControlAction as ProtoAction, HealthLiveness,
    HealthScope as ProtoScope, HealthScopeLevel, HealthScopeState, SetHealthControlRequest,
    SetHealthControlResponse, SetHealthControlResult,
};
use std::net::SocketAddr;
use std::sync::Arc;
use tonic::{Request, Response, Status};

#[derive(Clone)]
pub struct BackendHealthService {
    pub snapshots: Arc<SnapshotStore>,
}

fn health_to_proto(h: Health) -> i32 {
    match h {
        Health::Unknown => HealthLiveness::Unknown,
        Health::Up => HealthLiveness::Up,
        Health::Down => HealthLiveness::Down,
    }
    .into()
}

fn effective_scope_to_proto(s: EffectiveScope) -> i32 {
    match s {
        EffectiveScope::Automatic => HealthScopeState::Automatic,
        EffectiveScope::Frozen => HealthScopeState::Frozen,
    }
    .into()
}

#[allow(clippy::result_large_err)]
fn parse_scope(scope: ProtoScope, compiled: &CompiledHealth) -> Result<HealthControlScope, Status> {
    let level = HealthScopeLevel::try_from(scope.level)
        .map_err(|_| Status::invalid_argument("invalid scope level"))?;
    match level {
        HealthScopeLevel::Global => Ok(HealthControlScope::Global),
        HealthScopeLevel::Pool => {
            let pool = scope
                .pool
                .ok_or_else(|| Status::invalid_argument("pool scope requires pool"))?;
            Ok(HealthControlScope::Pool { pool })
        }
        HealthScopeLevel::Backend => {
            let pool = scope
                .pool
                .ok_or_else(|| Status::invalid_argument("backend scope requires pool"))?;
            let backend = scope
                .backend
                .ok_or_else(|| Status::invalid_argument("backend scope requires backend"))?;
            let address = compiled
                .resolve_backend(&pool, &backend)
                .map_err(Status::invalid_argument)?;
            Ok(HealthControlScope::Backend { pool, address })
        }
        HealthScopeLevel::Unspecified => Err(Status::invalid_argument("scope level required")),
    }
}

fn scope_to_proto(scope: &HealthControlScope) -> ProtoScope {
    match scope {
        HealthControlScope::Global => ProtoScope {
            level: HealthScopeLevel::Global.into(),
            pool: None,
            backend: None,
        },
        HealthControlScope::Pool { pool } => ProtoScope {
            level: HealthScopeLevel::Pool.into(),
            pool: Some(pool.clone()),
            backend: None,
        },
        HealthControlScope::Backend { pool, address } => ProtoScope {
            level: HealthScopeLevel::Backend.into(),
            pool: Some(pool.clone()),
            backend: Some(address.to_string()),
        },
    }
}

#[allow(clippy::result_large_err)]
fn parse_action(action: i32) -> Result<HealthControlAction, Status> {
    match ProtoAction::try_from(action).map_err(|_| Status::invalid_argument("invalid action"))? {
        ProtoAction::Freeze => Ok(HealthControlAction::Freeze),
        ProtoAction::SetUp => Ok(HealthControlAction::SetUp),
        ProtoAction::SetDown => Ok(HealthControlAction::SetDown),
        ProtoAction::ResumeAutomatic => Ok(HealthControlAction::ResumeAutomatic),
        ProtoAction::Unspecified => Err(Status::invalid_argument("action required")),
    }
}

#[allow(clippy::result_large_err)]
fn parse_filter(
    filter: Option<ProtoFilter>,
    compiled: &CompiledHealth,
) -> Result<BackendHealthFilter, Status> {
    let Some(filter) = filter else {
        return Ok(BackendHealthFilter::default());
    };
    let backend = match filter.backend {
        Some(ref id) => {
            if let Some(ref pool) = filter.pool {
                Some(
                    compiled
                        .resolve_backend(pool, id)
                        .map_err(Status::invalid_argument)?,
                )
            } else if let Ok(addr) = id.parse::<SocketAddr>() {
                Some(addr)
            } else {
                return Err(Status::invalid_argument(
                    "pool required when filtering by backend name",
                ));
            }
        }
        None => None,
    };
    Ok(BackendHealthFilter {
        pool: filter.pool,
        backend,
    })
}

#[tonic::async_trait]
impl BackendHealth for BackendHealthService {
    async fn get_backend_health(
        &self,
        request: Request<GetBackendHealthRequest>,
    ) -> Result<Response<GetBackendHealthResponse>, Status> {
        let snap = self.snapshots.load();
        if snap.health.is_empty() {
            return Ok(Response::new(GetBackendHealthResponse { entries: vec![] }));
        }
        let filter = parse_filter(request.into_inner().filter, &snap.health)?;
        let rows = self
            .snapshots
            .health()
            .backend_health_views(&snap.health, &filter);
        Ok(Response::new(GetBackendHealthResponse {
            entries: rows
                .into_iter()
                .map(|r| BackendHealthEntry {
                    pool: r.pool,
                    backend: r.backend.to_string(),
                    observed: health_to_proto(r.observed),
                    applied: health_to_proto(r.applied),
                    scope_state: effective_scope_to_proto(r.scope_state),
                    eligible: r.eligible,
                    latency_ewma_ms: r.latency_ewma_ms,
                    last_transition_unix_ms: r.last_transition_unix_ms,
                })
                .collect(),
        }))
    }

    async fn set_health_control(
        &self,
        request: Request<SetHealthControlRequest>,
    ) -> Result<Response<SetHealthControlResponse>, Status> {
        let req = request.into_inner();
        let action = parse_action(req.action)?;
        let snap = self.snapshots.load();
        if snap.health.is_empty() {
            return Err(Status::failed_precondition(
                "health checking not configured",
            ));
        }
        let scope = parse_scope(
            req.scope
                .ok_or_else(|| Status::invalid_argument("scope required"))?,
            &snap.health,
        )?;
        let outcomes = self
            .snapshots
            .health()
            .set_health_control(&snap.health, scope, action)
            .map_err(Status::invalid_argument)?;
        Ok(Response::new(SetHealthControlResponse {
            results: outcomes
                .into_iter()
                .map(|o| SetHealthControlResult {
                    scope: Some(scope_to_proto(&o.scope)),
                    scope_state: effective_scope_to_proto(o.scope_state),
                    pool: Some(o.pool),
                    backend: Some(o.backend.to_string()),
                    applied: health_to_proto(o.applied),
                })
                .collect(),
        }))
    }
}
