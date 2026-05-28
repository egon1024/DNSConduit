//! Exponential backoff for dnstap destination connect.

use conduit_proto::config::ConnectRetry;
use std::time::Duration;

/// Resolved connect-retry policy (defaults applied).
#[derive(Debug, Clone, PartialEq)]
pub struct ConnectRetryConfig {
    pub initial_ms: u32,
    pub max_ms: u32,
    pub multiplier: f64,
    pub max_elapsed_ms: u32,
    pub jitter: bool,
}

impl Default for ConnectRetryConfig {
    fn default() -> Self {
        Self {
            initial_ms: 1000,
            max_ms: 30_000,
            multiplier: 2.0,
            max_elapsed_ms: 0,
            jitter: true,
        }
    }
}

impl ConnectRetryConfig {
    pub fn resolve(opt: Option<&ConnectRetry>) -> Self {
        let Some(r) = opt else {
            return Self::default();
        };
        let defaults = Self::default();
        ConnectRetryConfig {
            initial_ms: if r.initial_ms == 0 {
                defaults.initial_ms
            } else {
                r.initial_ms
            },
            max_ms: if r.max_ms == 0 {
                defaults.max_ms
            } else {
                r.max_ms
            },
            multiplier: if r.multiplier == 0.0 {
                defaults.multiplier
            } else {
                r.multiplier
            },
            max_elapsed_ms: r.max_elapsed_ms,
            jitter: r.jitter,
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.initial_ms == 0 || self.max_ms == 0 {
            return Err("connect_retry initial_ms and max_ms must be > 0".into());
        }
        if self.multiplier < 1.0 {
            return Err("connect_retry multiplier must be >= 1.0".into());
        }
        Ok(())
    }

    /// Delay before attempt `failures` (0 = first retry after initial failure).
    pub fn delay_for_failure(&self, failures: u32) -> Duration {
        let base_ms = self.initial_ms as f64 * self.multiplier.powi(failures as i32);
        let capped = base_ms.min(self.max_ms as f64);
        let ms = if self.jitter {
            capped * jitter_factor()
        } else {
            capped
        };
        Duration::from_millis(ms.max(1.0) as u64)
    }
}

/// Backoff sequence state; reset after successful connect.
#[derive(Debug)]
pub struct BackoffState {
    config: ConnectRetryConfig,
    failures: u32,
}

impl BackoffState {
    pub fn new(config: ConnectRetryConfig) -> Self {
        Self {
            config,
            failures: 0,
        }
    }

    pub fn next_delay(&mut self) -> Duration {
        let delay = self.config.delay_for_failure(self.failures);
        self.failures = self.failures.saturating_add(1);
        delay
    }

    pub fn reset(&mut self) {
        self.failures = 0;
    }
}

/// Uniform factor in [0.75, 1.25] without pulling in `rand`.
fn jitter_factor() -> f64 {
    use std::collections::hash_map::RandomState;
    use std::hash::{BuildHasher, Hash, Hasher};
    use std::time::{SystemTime, UNIX_EPOCH};

    let mut hasher = RandomState::new().build_hasher();
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos()
        .hash(&mut hasher);
    std::thread::current().id().hash(&mut hasher);
    let h = hasher.finish();
    0.75 + (h % 5001) as f64 / 10_000.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_spec() {
        let c = ConnectRetryConfig::default();
        assert_eq!(c.initial_ms, 1000);
        assert_eq!(c.max_ms, 30_000);
        assert!((c.multiplier - 2.0).abs() < f64::EPSILON);
        assert!(c.jitter);
    }

    #[test]
    fn delay_grows_and_caps() {
        let c = ConnectRetryConfig {
            initial_ms: 250,
            max_ms: 8000,
            multiplier: 2.0,
            max_elapsed_ms: 0,
            jitter: false,
        };
        assert_eq!(c.delay_for_failure(0), Duration::from_millis(250));
        assert_eq!(c.delay_for_failure(1), Duration::from_millis(500));
        assert_eq!(c.delay_for_failure(10), Duration::from_millis(8000));
    }

    #[test]
    fn backoff_reset() {
        let c = ConnectRetryConfig::default();
        let mut b = BackoffState::new(c);
        let _ = b.next_delay();
        let _ = b.next_delay();
        assert_eq!(b.failures, 2);
        b.reset();
        assert_eq!(b.failures, 0);
    }

    #[test]
    fn reject_invalid_multiplier() {
        let c = ConnectRetryConfig {
            multiplier: 0.5,
            ..ConnectRetryConfig::default()
        };
        assert!(c.validate().is_err());
    }
}
