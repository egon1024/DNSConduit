//! Built-in Prometheus metrics (design §7.1).

use crate::compile::BuiltinProfile;
use prometheus::{
    Encoder, HistogramOpts, HistogramVec, IntCounterVec, Opts, Registry, TextEncoder,
};
pub struct BuiltinRegistry {
    enabled: bool,
    profile: BuiltinProfile,
    registry: Registry,
    queries_total: IntCounterVec,
    phase_duration: HistogramVec,
    forward_attempts: IntCounterVec,
    forward_errors: IntCounterVec,
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
            .buckets(prometheus::exponential_buckets(0.0001, 2.0, 16).expect("buckets")),
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
