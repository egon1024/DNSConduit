//! Backend pool member defaults and normalization (config contract).

use conduit_proto::config::Backend;

/// Default backend weight when `Backend.weight` is unset in config (YAML, proto, API).
pub const DEFAULT_BACKEND_WEIGHT: u32 = 100;

/// Effective weight for routing and validation.
pub fn effective_backend_weight(backend: &Backend) -> u32 {
    backend.weight.unwrap_or(DEFAULT_BACKEND_WEIGHT)
}
