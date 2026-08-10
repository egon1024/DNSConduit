//! Configuration load, validate, merge, and export.

pub mod backend;
pub mod dataplane;
pub mod defaults;
pub mod duration;
pub mod error;
pub mod export;
pub mod file;
pub mod forward;
pub mod health;
pub mod listeners;
pub mod logging;
pub mod lookup;
pub mod merge;
pub mod overlay;
pub mod shutdown;
pub mod size;
pub mod validate;

#[cfg(test)]
mod dataplane_tests;

pub use backend::{effective_backend_weight, DEFAULT_BACKEND_WEIGHT};
pub use conduit_proto::paths::resolve_config_path;
pub use dataplane::{
    effective_dataplane_runtime, effective_io_workers, effective_policy_workers,
    parse_dataplane_runtime, validate_dataplane, DEFAULT_DATAPLANE_RUNTIME, DEFAULT_IO_WORKERS,
    DEFAULT_POLICY_WORKERS,
};
pub use defaults::{
    DEFAULT_CONTROL_LISTEN_ADDRESS, DEFAULT_EVENTS_DROP_POLICY, DEFAULT_EVENTS_QUEUE_DEPTH,
    DEFAULT_FORWARD_OUTSTANDING_PER_BACKEND, DEFAULT_FORWARD_TIMEOUT_MS,
    DEFAULT_LISTENER_REUSE_PORT, DEFAULT_LISTENER_THREADS, DEFAULT_ORCHESTRATOR_MAX_ATTEMPTS,
    DEFAULT_ORCHESTRATOR_MAX_TXN_DURATION_MS, DEFAULT_ORCHESTRATOR_TXN_TABLE_CAPACITY,
    DEFAULT_RHAI_MAX_CALL_DEPTH, DEFAULT_RHAI_MAX_OPERATIONS,
};
pub use duration::parse_duration;
pub use error::ConfigError;
pub use export::export_yaml;
pub use file::{load_overlay_patch, load_yaml};
pub use forward::{
    compile_forward_from_config, parse_sources_v4, parse_sources_v6, parse_upstream_transport,
    validate_upstream_backend_addresses, CompiledForward, CompiledPoolForward, UpstreamTransport,
    DEFAULT_SOURCE_SELECTION, DEFAULT_UPSTREAM_TRANSPORT, MAX_SOURCES_V4, MAX_SOURCES_V6,
};
pub use health::{
    compile_health_from_config, validate_health, CompiledBackendHealth, CompiledHealth,
    CompiledPoolHealth, InitialHealthState, DEFAULT_HEALTH_FALL, DEFAULT_HEALTH_INTERVAL_MS,
    DEFAULT_HEALTH_PASSIVE_FALL, DEFAULT_HEALTH_RISE, DEFAULT_LATENCY_EWMA_ALPHA,
    DEFAULT_LATENCY_FLOOR, DEFAULT_PROBE_QNAME, DEFAULT_PROBE_QTYPE, HEALTH_INTERVAL_FLOOR_MS,
};
pub use listeners::{resolve_listener_ingress, ResolvedListenerIngress};
pub use logging::{init_from_config, validate_logging, DEFAULT_LOG_LEVEL, DEFAULT_LOG_OUTPUT};
pub use lookup::{
    compile_lookup_from_config, lookup_concurrency_for_lmdb_default, validate_lookup,
    CacheBackendType, CompiledCacheInstance, CompiledLmdbCache, CompiledLookup,
    CompiledLookupProfile, CompiledLookupProvider, CompiledMemoryCache, CompiledNegativeCache,
    CompiledTruncatedUdp, EvictionMode, LmdbSync, LmdbWhenFull, OnHitResponseRules,
    DEFAULT_LMDB_SAMPLE_SIZE, DEFAULT_LMDB_SYNC, DEFAULT_LOOKUP_PROFILE,
    DEFAULT_MEMORY_SHARD_COUNT, DEFAULT_ON_HIT_RESPONSE_RULES, DEFAULT_SERVFAIL_TTL_SECS,
    DEFAULT_TRUNCATED_UDP_TTL_SECS, LMDB_SYNC_INTERVAL_DEFAULT, LMDB_SYNC_INTERVAL_MAX,
    LMDB_SYNC_INTERVAL_MIN, MAX_LMDB_SHARD_COUNT,
};
pub use merge::{
    clear_overlay, is_overlay_patch_empty, merge_file_and_overlay, merge_overlay_patches,
    EffectiveConfig,
};
pub use overlay::validate_overlay_patch;
pub use shutdown::{
    effective_drain, effective_drain_timeout_ms, validate_shutdown, DEFAULT_DRAIN_ENABLED,
    DEFAULT_DRAIN_TIMEOUT_MS,
};
pub use size::parse_si_size;
pub use validate::{validate, ValidationResult};

use conduit_proto::config::Config;
use std::net::SocketAddr;

/// Parsed control-plane listen address when a `control` section is present.
pub fn control_listen_addr(cfg: &Config) -> Result<Option<SocketAddr>, ConfigError> {
    match cfg.control.as_ref() {
        None => Ok(None),
        Some(c) => {
            if c.listen_address.is_empty() {
                return Err(ConfigError::Invalid(
                    "control.listen_address must not be empty".into(),
                ));
            }
            c.listen_address.parse().map(Some).map_err(|e| {
                ConfigError::Invalid(format!(
                    "control.listen_address '{}': {e}",
                    c.listen_address
                ))
            })
        }
    }
}
