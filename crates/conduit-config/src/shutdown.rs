//! Process shutdown configuration helpers.
//!
//! Controls the graceful drain that runs before listener teardown: whether to
//! drain at all (`shutdown.drain`) and how long to wait (`shutdown.drain_timeout_ms`).
//! A second `SIGINT`/`SIGTERM` abandons the wait regardless of these settings.

use conduit_proto::config::Config;

/// Drain in-flight transactions before exit unless explicitly disabled.
pub const DEFAULT_DRAIN_ENABLED: bool = true;

/// Default drain wait when `shutdown.drain_timeout_ms` is unset (milliseconds).
pub const DEFAULT_DRAIN_TIMEOUT_MS: u32 = 5000;

/// Whether to drain in-flight transactions on shutdown (default: enabled).
pub fn effective_drain(cfg: &Config) -> bool {
    cfg.shutdown
        .as_ref()
        .and_then(|s| s.drain)
        .unwrap_or(DEFAULT_DRAIN_ENABLED)
}

/// Maximum drain wait in milliseconds (default: [`DEFAULT_DRAIN_TIMEOUT_MS`]).
pub fn effective_drain_timeout_ms(cfg: &Config) -> u32 {
    cfg.shutdown
        .as_ref()
        .and_then(|s| s.drain_timeout_ms)
        .unwrap_or(DEFAULT_DRAIN_TIMEOUT_MS)
}

/// Validate the `shutdown` block. No constraints today: `drain_timeout_ms: 0`
/// is a valid "single immediate check" and `drain: false` disables draining.
pub fn validate_shutdown(_cfg: &Config) -> Vec<String> {
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;
    use conduit_proto::config::ShutdownConfig;

    #[test]
    fn defaults_when_block_absent() {
        let cfg = Config::default();
        assert!(effective_drain(&cfg));
        assert_eq!(effective_drain_timeout_ms(&cfg), DEFAULT_DRAIN_TIMEOUT_MS);
    }

    #[test]
    fn explicit_values_override_defaults() {
        let cfg = Config {
            shutdown: Some(ShutdownConfig {
                drain: Some(false),
                drain_timeout_ms: Some(250),
            }),
            ..Default::default()
        };
        assert!(!effective_drain(&cfg));
        assert_eq!(effective_drain_timeout_ms(&cfg), 250);
    }

    #[test]
    fn partial_block_keeps_other_default() {
        let cfg = Config {
            shutdown: Some(ShutdownConfig {
                drain: None,
                drain_timeout_ms: Some(1000),
            }),
            ..Default::default()
        };
        assert!(effective_drain(&cfg));
        assert_eq!(effective_drain_timeout_ms(&cfg), 1000);
    }
}
