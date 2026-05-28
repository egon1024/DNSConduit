//! Built-in Prometheus metrics (design §7.1).

use crate::compile::BuiltinProfile;
use prometheus::{
    Encoder, HistogramOpts, HistogramVec, IntCounterVec, Opts, Registry, TextEncoder,
};

/// Cumulative histogram upper bounds (seconds) for upstream forward RTT.
/// Aligns with familiar DNS latency bands (1 ms, 10 ms, 50 ms, …).
fn forward_duration_buckets() -> Vec<f64> {
    vec![0.001, 0.01, 0.05, 0.1, 0.5, 1.0, 5.0, 10.0]
}

/// Cumulative histogram upper bounds (seconds) for orchestrator phase time.
/// Includes 100 µs for fast in-process phases, then the same bands as forward RTT.
fn phase_duration_buckets() -> Vec<f64> {
    vec![0.0001, 0.001, 0.01, 0.05, 0.1, 0.5, 1.0, 5.0, 10.0]
}
pub struct BuiltinRegistry {
    enabled: bool,
    profile: BuiltinProfile,
    registry: Registry,
    queries_total: IntCounterVec,
    phase_duration: HistogramVec,
    forward_attempts: IntCounterVec,
    forward_errors: IntCounterVec,
    forward_duration: HistogramVec,
    retries_total: IntCounterVec,
}

impl BuiltinRegistry {
    pub fn new(enabled: bool, profile: BuiltinProfile) -> Self {
        let registry = Registry::new();
        let queries_total = IntCounterVec::new(
            Opts::new("conduit_queries_total", "DNS queries received"),
            &["listener", "protocol"],
        )
        .expect("metric");
        let phase_duration = HistogramVec::new(
            HistogramOpts::new(
                "conduit_phase_duration_seconds",
                "Time spent in orchestrator phase",
            )
            .buckets(phase_duration_buckets()),
            &["phase"],
        )
        .expect("metric");
        let forward_attempts = IntCounterVec::new(
            Opts::new(
                "conduit_forward_attempts_total",
                "Upstream forward attempts",
            ),
            &["pool", "backend", "outcome"],
        )
        .expect("metric");
        let forward_errors = IntCounterVec::new(
            Opts::new("conduit_forward_errors_total", "Forward errors"),
            &["pool", "reason"],
        )
        .expect("metric");
        let forward_duration = HistogramVec::new(
            HistogramOpts::new(
                "conduit_forward_duration_seconds",
                "Upstream forward round-trip time",
            )
            .buckets(forward_duration_buckets()),
            &["pool", "backend"],
        )
        .expect("metric");
        let retries_total = IntCounterVec::new(
            Opts::new("conduit_retries_total", "Retry transitions"),
            &["pool"],
        )
        .expect("metric");
        registry
            .register(Box::new(queries_total.clone()))
            .expect("register");
        registry
            .register(Box::new(phase_duration.clone()))
            .expect("register");
        registry
            .register(Box::new(forward_attempts.clone()))
            .expect("register");
        registry
            .register(Box::new(forward_errors.clone()))
            .expect("register");
        registry
            .register(Box::new(forward_duration.clone()))
            .expect("register");
        registry
            .register(Box::new(retries_total.clone()))
            .expect("register");
        Self {
            enabled: enabled && profile != BuiltinProfile::Off,
            profile,
            registry,
            queries_total,
            phase_duration,
            forward_attempts,
            forward_errors,
            forward_duration,
            retries_total,
        }
    }

    pub fn enabled(&self) -> bool {
        self.enabled
    }

    pub fn profile(&self) -> BuiltinProfile {
        self.profile
    }

    pub fn record_query(&self, listener: &str, protocol: &str) {
        if !self.enabled {
            return;
        }
        self.queries_total
            .with_label_values(&[listener, protocol])
            .inc();
    }

    pub fn observe_phase(&self, phase: &str, duration_secs: f64) {
        if !self.enabled || self.profile == BuiltinProfile::Minimal {
            return;
        }
        self.phase_duration
            .with_label_values(&[phase])
            .observe(duration_secs);
    }

    pub fn record_forward_attempt(&self, pool: &str, backend: &str, outcome: &str) {
        if !self.enabled || self.profile == BuiltinProfile::Minimal {
            return;
        }
        self.forward_attempts
            .with_label_values(&[pool, backend, outcome])
            .inc();
    }

    pub fn record_forward_error(&self, pool: &str, reason: &str) {
        if !self.enabled || self.profile == BuiltinProfile::Minimal {
            return;
        }
        self.forward_errors.with_label_values(&[pool, reason]).inc();
    }

    pub fn record_forward_duration(&self, pool: &str, backend: &str, duration_secs: f64) {
        if !self.enabled || self.profile == BuiltinProfile::Minimal {
            return;
        }
        self.forward_duration
            .with_label_values(&[pool, backend])
            .observe(duration_secs);
    }

    pub fn record_retry(&self, pool: &str) {
        if !self.enabled || self.profile == BuiltinProfile::Minimal {
            return;
        }
        self.retries_total.with_label_values(&[pool]).inc();
    }

    pub fn gather(&self) -> Vec<prometheus::proto::MetricFamily> {
        self.registry.gather()
    }
}

pub fn encode_builtin(families: Vec<prometheus::proto::MetricFamily>) -> String {
    let encoder = TextEncoder::new();
    let mut buf = Vec::new();
    encoder.encode(&families, &mut buf).expect("encode");
    String::from_utf8(buf).expect("utf8")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compile::BuiltinProfile;

    #[test]
    fn forward_duration_buckets_are_strictly_increasing() {
        let buckets = forward_duration_buckets();
        assert!(buckets.windows(2).all(|w| w[0] < w[1]));
    }

    #[test]
    fn scrape_includes_human_friendly_forward_duration_le_labels() {
        let reg = BuiltinRegistry::new(true, BuiltinProfile::Full);
        reg.record_forward_duration("default", "127.0.0.1:5300", 0.001);
        let body = encode_builtin(reg.gather());
        for le in [
            "0.001", "0.01", "0.05", "0.1", "0.5", "1", "5", "10", "+Inf",
        ] {
            assert!(
                body.contains(&format!(r#"le="{le}""#)),
                "missing le={le} in:\n{body}"
            );
        }
    }
}
