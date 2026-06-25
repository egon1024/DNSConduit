//! Configuration load, validate, merge, and export.

pub mod backend;
pub mod dataplane;
pub mod defaults;
pub mod error;
pub mod export;
pub mod file;
pub mod forward;
pub mod listeners;
pub mod logging;
pub mod merge;
pub mod overlay;
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
pub use error::ConfigError;
pub use export::export_yaml;
pub use file::{load_overlay_patch, load_yaml};
pub use forward::{
    compile_forward_from_config, parse_sources_v4, parse_sources_v6, parse_upstream_transport,
    validate_upstream_backend_addresses, CompiledForward, CompiledPoolForward, UpstreamTransport,
    DEFAULT_SOURCE_SELECTION, DEFAULT_UPSTREAM_TRANSPORT, MAX_SOURCES_V4, MAX_SOURCES_V6,
};
pub use listeners::{resolve_listener_ingress, ResolvedListenerIngress};
pub use logging::{init_from_config, validate_logging, DEFAULT_LOG_LEVEL, DEFAULT_LOG_OUTPUT};
pub use merge::{
    clear_overlay, is_overlay_patch_empty, merge_file_and_overlay, merge_overlay_patches,
    EffectiveConfig,
};
pub use overlay::validate_overlay_patch;
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
