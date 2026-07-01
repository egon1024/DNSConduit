//! Operator health controls: tri-state scope precedence (design §D8).
//!
//! Scope is settable at global, pool, and backend levels as `inherit` (absence),
//! `frozen`, or `automatic`. Resolution is most-specific-wins:
//! backend → pool → global → system default (`automatic`).

use super::state::{BackendHealthState, BackendKey, Health, HealthControlScopes, HealthRegistry};
use conduit_config::health::CompiledHealth;
use std::net::SocketAddr;
use std::sync::atomic::Ordering;

const SCOPE_AUTOMATIC: u8 = 0;
const SCOPE_FROZEN: u8 = 1;

/// Tri-state scope setting stored at a control level (design §D8).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScopeMode {
    /// Follow the next broader level (not stored in maps).
    Inherit,
    /// Hold `applied` while `observed` keeps updating.
    Frozen,
    /// Probe-driven: `applied` tracks `observed`.
    Automatic,
}

/// Effective scope after most-specific-wins resolution.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EffectiveScope {
    Frozen,
    Automatic,
}

/// Control scope target for `SetHealthControl`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HealthControlScope {
    Global,
    Pool { pool: String },
    Backend { pool: String, address: SocketAddr },
}

/// Control-plane action (maps 1:1 to the proto enum).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HealthControlAction {
    Freeze,
    SetUp,
    SetDown,
    ResumeAutomatic,
}

/// One backend row returned by `GetBackendHealth`.
#[derive(Debug, Clone)]
pub struct BackendHealthView {
    pub pool: String,
    pub backend: SocketAddr,
    pub observed: Health,
    pub applied: Health,
    pub scope_state: EffectiveScope,
    pub eligible: bool,
    pub latency_ewma_ms: Option<f64>,
    pub last_transition_unix_ms: Option<u64>,
}

/// One backend affected by `SetHealthControl`.
#[derive(Debug, Clone)]
pub struct HealthControlOutcome {
    pub scope: HealthControlScope,
    pub scope_state: EffectiveScope,
    pub pool: String,
    pub backend: SocketAddr,
    pub applied: Health,
}

/// Optional filter for `GetBackendHealth`.
#[derive(Debug, Clone, Default)]
pub struct BackendHealthFilter {
    pub pool: Option<String>,
    pub backend: Option<SocketAddr>,
}

impl HealthControlScopes {
    pub(crate) fn set_level(&self, scope: &HealthControlScope, mode: ScopeMode) {
        match scope {
            HealthControlScope::Global => {
                self.global.store(scope_to_u8(mode), Ordering::Relaxed);
            }
            HealthControlScope::Pool { pool } => {
                let mut pools = self.pools.write().expect("scope lock");
                match mode {
                    ScopeMode::Inherit => {
                        pools.remove(pool);
                    }
                    other => {
                        pools.insert(pool.clone(), scope_to_u8(other));
                    }
                }
            }
            HealthControlScope::Backend { pool, address } => {
                let key = BackendKey::new(pool.clone(), *address);
                let mut backends = self.backends.write().expect("scope lock");
                match mode {
                    ScopeMode::Inherit => {
                        backends.remove(&key);
                    }
                    other => {
                        backends.insert(key, scope_to_u8(other));
                    }
                }
            }
        }
    }

    pub(crate) fn resolve(&self, pool: &str, key: &BackendKey) -> EffectiveScope {
        if let Some(mode) = self.explicit_at_backend(key) {
            return mode.into();
        }
        if let Some(mode) = self.explicit_at_pool(pool) {
            return mode.into();
        }
        self.explicit_global().into()
    }

    fn explicit_at_backend(&self, key: &BackendKey) -> Option<ScopeMode> {
        self.backends
            .read()
            .expect("scope lock")
            .get(key)
            .copied()
            .map(u8_to_scope)
    }

    fn explicit_at_pool(&self, pool: &str) -> Option<ScopeMode> {
        self.pools
            .read()
            .expect("scope lock")
            .get(pool)
            .copied()
            .map(u8_to_scope)
    }

    fn explicit_global(&self) -> ScopeMode {
        u8_to_scope(self.global.load(Ordering::Relaxed))
    }
}

impl From<ScopeMode> for EffectiveScope {
    fn from(mode: ScopeMode) -> Self {
        match mode {
            ScopeMode::Frozen => EffectiveScope::Frozen,
            ScopeMode::Inherit | ScopeMode::Automatic => EffectiveScope::Automatic,
        }
    }
}

fn scope_to_u8(mode: ScopeMode) -> u8 {
    match mode {
        ScopeMode::Automatic | ScopeMode::Inherit => SCOPE_AUTOMATIC,
        ScopeMode::Frozen => SCOPE_FROZEN,
    }
}

fn u8_to_scope(v: u8) -> ScopeMode {
    match v {
        SCOPE_FROZEN => ScopeMode::Frozen,
        _ => ScopeMode::Automatic,
    }
}

impl HealthRegistry {
    /// Resolve the effective scope for one backend (design §D8).
    pub fn resolve_scope(&self, pool: &str, address: SocketAddr) -> EffectiveScope {
        let key = BackendKey::new(pool, address);
        self.scopes.resolve(pool, &key)
    }

    /// Whether probe-driven transitions may move `applied` for this backend.
    pub fn is_effectively_frozen(&self, pool: &str, address: SocketAddr) -> bool {
        self.resolve_scope(pool, address) == EffectiveScope::Frozen
    }

    /// Sync the per-backend frozen flag from resolved scope (called after control
    /// changes and before listing state).
    pub fn sync_frozen_flags(&self, compiled: &CompiledHealth) {
        let table = self.table.load();
        for (pool_name, pool) in &compiled.pools {
            for backend in &pool.backends {
                let key = BackendKey::new(pool_name.clone(), backend.address);
                if let Some(state) = table.get(&key) {
                    let frozen = self.is_effectively_frozen(pool_name, backend.address);
                    state.set_frozen_flag(frozen);
                }
            }
        }
    }

    fn sync_one(&self, pool: &str, address: SocketAddr, state: &BackendHealthState) {
        state.set_frozen_flag(self.is_effectively_frozen(pool, address));
    }

    /// List backend health rows, optionally filtered.
    pub fn backend_health_views(
        &self,
        compiled: &CompiledHealth,
        filter: &BackendHealthFilter,
    ) -> Vec<BackendHealthView> {
        self.sync_frozen_flags(compiled);
        let table = self.table.load();
        let mut rows = Vec::new();
        for (pool_name, pool) in &compiled.pools {
            if let Some(ref want_pool) = filter.pool {
                if want_pool != pool_name {
                    continue;
                }
            }
            for backend in &pool.backends {
                if let Some(want_addr) = filter.backend {
                    if backend.address != want_addr {
                        continue;
                    }
                }
                let key = BackendKey::new(pool_name.clone(), backend.address);
                let Some(state) = table.get(&key) else {
                    continue;
                };
                let observed = state.observed();
                let applied = state.applied();
                rows.push(BackendHealthView {
                    pool: pool_name.clone(),
                    backend: backend.address,
                    observed,
                    applied,
                    scope_state: self.resolve_scope(pool_name, backend.address),
                    eligible: applied == Health::Up,
                    latency_ewma_ms: state.latency_ewma_ms(),
                    last_transition_unix_ms: state.last_transition_unix_ms(),
                });
            }
        }
        rows
    }

    /// Apply an operator control action at the given scope.
    pub fn set_health_control(
        &self,
        compiled: &CompiledHealth,
        scope: HealthControlScope,
        action: HealthControlAction,
    ) -> Result<Vec<HealthControlOutcome>, String> {
        self.validate_scope(&scope, compiled)?;
        let affected = self.backends_in_scope(&scope, compiled);
        if affected.is_empty() {
            return Err("no backends in scope".into());
        }

        match action {
            HealthControlAction::Freeze => {
                self.scopes.set_level(&scope, ScopeMode::Frozen);
            }
            HealthControlAction::ResumeAutomatic => {
                self.scopes.set_level(&scope, ScopeMode::Automatic);
            }
            HealthControlAction::SetUp | HealthControlAction::SetDown => {
                // Manual set implies freeze at this scope (design §D2).
                self.scopes.set_level(&scope, ScopeMode::Frozen);
            }
        }

        let table = self.table.load();
        let mut outcomes = Vec::with_capacity(affected.len());
        for (pool, address) in affected {
            let key = BackendKey::new(pool.clone(), address);
            let Some(state) = table.get(&key) else {
                continue;
            };
            match action {
                HealthControlAction::Freeze => {}
                HealthControlAction::SetUp => state.set_applied_only(Health::Up),
                HealthControlAction::SetDown => state.set_applied_only(Health::Down),
                HealthControlAction::ResumeAutomatic => {
                    if self.resolve_scope(&pool, address) == EffectiveScope::Automatic {
                        state.snap_applied_to_observed();
                    }
                }
            }
            self.sync_one(&pool, address, state);
            outcomes.push(HealthControlOutcome {
                scope: scope_for_backend(&scope, &pool, address),
                scope_state: self.resolve_scope(&pool, address),
                pool,
                backend: address,
                applied: state.applied(),
            });
        }
        Ok(outcomes)
    }

    fn validate_scope(
        &self,
        scope: &HealthControlScope,
        compiled: &CompiledHealth,
    ) -> Result<(), String> {
        match scope {
            HealthControlScope::Global => Ok(()),
            HealthControlScope::Pool { pool } => {
                if compiled.pool(pool).is_some() {
                    Ok(())
                } else {
                    Err(format!("unknown pool {pool:?}"))
                }
            }
            HealthControlScope::Backend { pool, address } => {
                let Some(pool_cfg) = compiled.pool(pool) else {
                    return Err(format!("unknown pool {pool:?}"));
                };
                if pool_cfg.backends.iter().any(|b| b.address == *address) {
                    Ok(())
                } else {
                    Err(format!("backend {address} not in pool {pool:?}"))
                }
            }
        }
    }

    fn backends_in_scope(
        &self,
        scope: &HealthControlScope,
        compiled: &CompiledHealth,
    ) -> Vec<(String, SocketAddr)> {
        match scope {
            HealthControlScope::Global => compiled
                .pools
                .iter()
                .flat_map(|(pool, p)| {
                    p.backends
                        .iter()
                        .map(|b| (pool.clone(), b.address))
                        .collect::<Vec<_>>()
                })
                .collect(),
            HealthControlScope::Pool { pool } => compiled
                .pool(pool)
                .map(|p| {
                    p.backends
                        .iter()
                        .map(|b| (pool.clone(), b.address))
                        .collect()
                })
                .unwrap_or_default(),
            HealthControlScope::Backend { pool, address } => {
                vec![(pool.clone(), *address)]
            }
        }
    }
}

fn scope_for_backend(
    request_scope: &HealthControlScope,
    pool: &str,
    address: SocketAddr,
) -> HealthControlScope {
    match request_scope {
        HealthControlScope::Global | HealthControlScope::Pool { .. } => {
            HealthControlScope::Backend {
                pool: pool.to_string(),
                address,
            }
        }
        HealthControlScope::Backend { .. } => request_scope.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use conduit_config::health::{CompiledBackendHealth, CompiledPoolHealth, InitialHealthState};
    use std::collections::HashMap;

    fn compiled_pool(backends: Vec<&str>) -> CompiledHealth {
        let pool = CompiledPoolHealth {
            interval_ms: 1000,
            timeout_ms: 1000,
            rise: 3,
            fall: 2,
            acceptable_rcodes: None,
            initial_state: InitialHealthState::Optimistic,
            latency_weighting: false,
            latency_ewma_alpha: 0.2,
            latency_floor: 0.25,
            min_eligible: 0,
            passive_fast_trip: true,
            passive_fall: 2,
            backends: backends
                .into_iter()
                .map(|a| CompiledBackendHealth {
                    address: a.parse().unwrap(),
                    name: None,
                    label: a.into(),
                    probe_qname: "health.".into(),
                    probe_qtype: 1,
                    probe_source: None,
                })
                .collect(),
        };
        let mut pools = HashMap::new();
        pools.insert("default".to_string(), pool);
        CompiledHealth { pools }
    }

    fn addr(s: &str) -> SocketAddr {
        s.parse().unwrap()
    }

    #[test]
    fn scope_resolution_table() {
        let compiled = compiled_pool(vec!["127.0.0.1:5300", "127.0.0.1:5301"]);
        let reg = HealthRegistry::from_compiled(&compiled);
        let b0 = addr("127.0.0.1:5300");
        let b1 = addr("127.0.0.1:5301");

        assert_eq!(reg.resolve_scope("default", b0), EffectiveScope::Automatic);

        reg.scopes
            .set_level(&HealthControlScope::Global, ScopeMode::Frozen);
        assert_eq!(reg.resolve_scope("default", b0), EffectiveScope::Frozen);
        assert_eq!(reg.resolve_scope("default", b1), EffectiveScope::Frozen);

        reg.scopes.set_level(
            &HealthControlScope::Backend {
                pool: "default".into(),
                address: b1,
            },
            ScopeMode::Automatic,
        );
        assert_eq!(reg.resolve_scope("default", b0), EffectiveScope::Frozen);
        assert_eq!(
            reg.resolve_scope("default", b1),
            EffectiveScope::Automatic,
            "backend carve-out beats global freeze"
        );

        reg.scopes.set_level(
            &HealthControlScope::Pool {
                pool: "default".into(),
            },
            ScopeMode::Automatic,
        );
        assert_eq!(
            reg.resolve_scope("default", b0),
            EffectiveScope::Automatic,
            "pool automatic beats global frozen"
        );
    }

    #[test]
    fn manual_set_implies_freeze() {
        let compiled = compiled_pool(vec!["127.0.0.1:5300"]);
        let reg = HealthRegistry::from_compiled(&compiled);
        let b0 = addr("127.0.0.1:5300");
        let state = reg.get("default", b0).unwrap();

        reg.set_health_control(
            &compiled,
            HealthControlScope::Backend {
                pool: "default".into(),
                address: b0,
            },
            HealthControlAction::SetDown,
        )
        .unwrap();

        assert_eq!(reg.resolve_scope("default", b0), EffectiveScope::Frozen);
        assert_eq!(state.applied(), Health::Down);
        state.record_success(1, 0.2, 1.0);
        assert_eq!(state.observed(), Health::Up);
        assert_eq!(state.applied(), Health::Down, "frozen applied holds");
    }

    #[test]
    fn resume_automatic_snaps_to_observed() {
        let compiled = compiled_pool(vec!["127.0.0.1:5300"]);
        let reg = HealthRegistry::from_compiled(&compiled);
        let b0 = addr("127.0.0.1:5300");
        let state = reg.get("default", b0).unwrap();

        reg.set_health_control(
            &compiled,
            HealthControlScope::Backend {
                pool: "default".into(),
                address: b0,
            },
            HealthControlAction::SetDown,
        )
        .unwrap();
        state.record_success(1, 0.2, 1.0);
        assert_eq!(state.observed(), Health::Up);
        assert_eq!(state.applied(), Health::Down);

        reg.set_health_control(
            &compiled,
            HealthControlScope::Backend {
                pool: "default".into(),
                address: b0,
            },
            HealthControlAction::ResumeAutomatic,
        )
        .unwrap();

        assert_eq!(reg.resolve_scope("default", b0), EffectiveScope::Automatic);
        assert_eq!(
            state.applied(),
            Health::Up,
            "resume snaps applied := observed"
        );
    }

    #[test]
    fn clear_while_frozen_guard_resume_is_blessed_path() {
        let compiled = compiled_pool(vec!["127.0.0.1:5300"]);
        let reg = HealthRegistry::from_compiled(&compiled);
        let b0 = addr("127.0.0.1:5300");
        let state = reg.get("default", b0).unwrap();

        reg.set_health_control(
            &compiled,
            HealthControlScope::Backend {
                pool: "default".into(),
                address: b0,
            },
            HealthControlAction::SetDown,
        )
        .unwrap();
        state.record_success(1, 0.2, 1.0);
        assert_eq!(state.observed(), Health::Up);

        // Naive "clear frozen" without snap would leave stale applied=Down while
        // observed=Up — the footgun documented in design §D8.
        state.set_frozen_flag(false);
        assert!(!state.is_frozen());
        assert_eq!(
            state.applied(),
            Health::Down,
            "unfreezing alone does not repair stale applied"
        );

        reg.set_health_control(
            &compiled,
            HealthControlScope::Backend {
                pool: "default".into(),
                address: b0,
            },
            HealthControlAction::ResumeAutomatic,
        )
        .unwrap();
        assert_eq!(state.applied(), Health::Up);
    }

    #[test]
    fn set_control_via_resolved_backend_name() {
        let pool = CompiledPoolHealth {
            interval_ms: 1000,
            timeout_ms: 1000,
            rise: 3,
            fall: 2,
            acceptable_rcodes: None,
            initial_state: InitialHealthState::Optimistic,
            latency_weighting: false,
            latency_ewma_alpha: 0.2,
            latency_floor: 0.25,
            min_eligible: 0,
            passive_fast_trip: true,
            passive_fall: 2,
            backends: vec![
                CompiledBackendHealth {
                    address: addr("127.0.0.1:5300"),
                    name: Some("live".into()),
                    label: "live".into(),
                    probe_qname: "health.".into(),
                    probe_qtype: 1,
                    probe_source: None,
                },
                CompiledBackendHealth {
                    address: addr("127.0.0.1:5301"),
                    name: Some("dead".into()),
                    label: "dead".into(),
                    probe_qname: "health.".into(),
                    probe_qtype: 1,
                    probe_source: None,
                },
            ],
        };
        let mut pools = HashMap::new();
        pools.insert("default".to_string(), pool);
        let compiled = CompiledHealth { pools };

        let resolved = compiled.resolve_backend("default", "dead").unwrap();
        assert_eq!(resolved, addr("127.0.0.1:5301"));

        let reg = HealthRegistry::from_compiled(&compiled);
        reg.set_health_control(
            &compiled,
            HealthControlScope::Backend {
                pool: "default".into(),
                address: resolved,
            },
            HealthControlAction::SetDown,
        )
        .unwrap();
        assert_eq!(
            reg.get("default", resolved).unwrap().applied(),
            Health::Down
        );
    }
}
