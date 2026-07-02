//! Read-only routing runtime snapshots for Rhai `runtime.routing` (design §D3).

use std::collections::HashMap;

/// Per-backend routing/health view at hook entry.
#[derive(Debug, Clone, PartialEq)]
pub struct BackendRoutingView {
    /// `true` when the pool/backend exists in the active snapshot.
    pub configured: bool,
    pub applied: &'static str,
    pub observed: &'static str,
    pub eligible: bool,
    pub frozen: bool,
    pub latency_ewma_ms: Option<f64>,
    pub weight_factor: f64,
    pub outstanding: u32,
    pub last_transition_unix_ms: Option<u64>,
}

impl BackendRoutingView {
    pub const EMPTY: Self = Self {
        configured: false,
        applied: "unknown",
        observed: "unknown",
        eligible: false,
        frozen: false,
        latency_ewma_ms: None,
        weight_factor: 1.0,
        outstanding: 0,
        last_transition_unix_ms: None,
    };
}

/// Per-pool routing/health aggregate at hook entry.
#[derive(Debug, Clone, PartialEq)]
pub struct PoolRoutingView {
    pub configured: bool,
    pub configured_count: u32,
    pub eligible_count: u32,
    pub fail_open_active: bool,
    pub min_latency_ewma_ms: Option<f64>,
    pub max_outstanding: u32,
}

impl PoolRoutingView {
    pub const EMPTY: Self = Self {
        configured: false,
        configured_count: 0,
        eligible_count: 0,
        fail_open_active: false,
        min_latency_ewma_ms: None,
        max_outstanding: 0,
    };
}

/// Immutable routing runtime snapshot built once per script hook invocation.
#[derive(Debug, Clone, Default)]
pub struct RoutingRuntimeSnapshot {
    pub config_generation: u64,
    pools: HashMap<String, PoolRoutingView>,
    backends: HashMap<(String, String), BackendRoutingView>,
}

impl RoutingRuntimeSnapshot {
    pub fn new(
        config_generation: u64,
        pools: HashMap<String, PoolRoutingView>,
        backends: HashMap<(String, String), BackendRoutingView>,
    ) -> Self {
        Self {
            config_generation,
            pools,
            backends,
        }
    }

    pub fn pool(&self, name: &str) -> PoolRoutingView {
        self.pools
            .get(name)
            .cloned()
            .unwrap_or(PoolRoutingView::EMPTY)
    }

    pub fn backend(&self, pool: &str, id: &str) -> BackendRoutingView {
        self.backends
            .get(&(pool.to_string(), id.to_string()))
            .cloned()
            .unwrap_or(BackendRoutingView::EMPTY)
    }

    pub fn config_generation(&self) -> u64 {
        self.config_generation
    }
}
