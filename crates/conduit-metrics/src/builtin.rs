//! Built-in Prometheus metrics (design §7.1).

use crate::compile::BuiltinProfile;
use crate::labels::{ip_family_label, qclass_label, qtype_label, rcode_class_label, rcode_label};
use parking_lot::RwLock;
use prometheus::{
    Encoder, Gauge, GaugeVec, HistogramOpts, HistogramVec, IntCounterVec, IntGauge, Opts, Registry,
    TextEncoder,
};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

/// Cumulative histogram upper bounds (seconds) for upstream forward RTT.
fn forward_duration_buckets() -> Vec<f64> {
    vec![0.001, 0.01, 0.05, 0.1, 0.5, 1.0, 5.0, 10.0]
}

/// Cumulative histogram upper bounds (seconds) for orchestrator phase time.
fn phase_duration_buckets() -> Vec<f64> {
    vec![0.0001, 0.001, 0.01, 0.05, 0.1, 0.5, 1.0, 5.0, 10.0]
}

/// Snapshot data for scrape-time gauges (off worker threads).
#[derive(Debug, Clone, Default)]
pub struct ScrapeGaugeSnapshot {
    pub config_generation: u64,
    pub pool_backend_counts: Vec<(String, u32)>,
    pub forward_outstanding: Vec<(String, String, u32)>,
}

pub type ScrapeSnapshotFn = Arc<dyn Fn() -> ScrapeGaugeSnapshot + Send + Sync>;

enum QueriesTotal {
    Minimal(IntCounterVec),
    Full(IntCounterVec),
}

enum ResponsesTotal {
    Minimal(IntCounterVec),
    Full(IntCounterVec),
}

pub struct BuiltinRegistry {
    enabled: bool,
    profile: BuiltinProfile,
    registry: Registry,
    queries_total: QueriesTotal,
    responses_total: Option<ResponsesTotal>,
    parse_rejected_total: Option<IntCounterVec>,
    queries_by_pool_total: IntCounterVec,
    phase_duration: HistogramVec,
    forward_attempts: IntCounterVec,
    forward_errors: IntCounterVec,
    forward_duration: HistogramVec,
    retries_total: IntCounterVec,
    forward_outstanding: GaugeVec,
    pool_backends_configured: GaugeVec,
    #[allow(dead_code)]
    build_info: IntGauge,
    #[allow(dead_code)]
    start_time_seconds: Gauge,
    config_generation: Gauge,
    process_resident_bytes: Option<Gauge>,
    process_open_fds: Option<Gauge>,
    scrape_fn: RwLock<Option<ScrapeSnapshotFn>>,
}

impl BuiltinRegistry {
    pub fn new(enabled: bool, profile: BuiltinProfile) -> Self {
        let registry = Registry::new();
        let effective = enabled && profile != BuiltinProfile::Off;
        let is_full = profile == BuiltinProfile::Full;

        let queries_total = if is_full {
            let v = IntCounterVec::new(
                Opts::new("conduit_queries_total", "DNS queries received"),
                &["listener", "protocol", "qtype", "qclass", "ip_family"],
            )
            .expect("metric");
            registry.register(Box::new(v.clone())).expect("register");
            QueriesTotal::Full(v)
        } else {
            let v = IntCounterVec::new(
                Opts::new("conduit_queries_total", "DNS queries received"),
                &["listener", "protocol"],
            )
            .expect("metric");
            registry.register(Box::new(v.clone())).expect("register");
            QueriesTotal::Minimal(v)
        };

        let responses_total = if effective {
            if is_full {
                let v = IntCounterVec::new(
                    Opts::new("conduit_responses_total", "DNS responses sent to clients"),
                    &["listener", "protocol", "rcode", "ip_family"],
                )
                .expect("metric");
                registry.register(Box::new(v.clone())).expect("register");
                Some(ResponsesTotal::Full(v))
            } else {
                let v = IntCounterVec::new(
                    Opts::new("conduit_responses_total", "DNS responses sent to clients"),
                    &["listener", "protocol", "rcode"],
                )
                .expect("metric");
                registry.register(Box::new(v.clone())).expect("register");
                Some(ResponsesTotal::Minimal(v))
            }
        } else {
            None
        };

        let parse_rejected_total = if effective && is_full {
            let v = IntCounterVec::new(
                Opts::new(
                    "conduit_parse_rejected_total",
                    "Queries rejected at parse stage",
                ),
                &["reason"],
            )
            .expect("metric");
            registry.register(Box::new(v.clone())).expect("register");
            Some(v)
        } else {
            None
        };

        let queries_by_pool_total = IntCounterVec::new(
            Opts::new(
                "conduit_queries_by_pool_total",
                "Queries after route selection by pool",
            ),
            &["pool"],
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
        let forward_outstanding = GaugeVec::new(
            Opts::new(
                "conduit_forward_outstanding",
                "In-flight upstream forwards per backend",
            ),
            &["pool", "backend"],
        )
        .expect("metric");
        let pool_backends_configured = GaugeVec::new(
            Opts::new(
                "conduit_pool_backends_configured",
                "Configured backends per pool",
            ),
            &["pool"],
        )
        .expect("metric");

        let mut build_info_opts = Opts::new("conduit_build_info", "Build information");
        for (name, value) in crate::build_metadata::label_pairs() {
            build_info_opts = build_info_opts.const_label(name, value);
        }
        let build_info = IntGauge::with_opts(build_info_opts).expect("metric");
        build_info.set(1);

        let start_time_seconds = Gauge::with_opts(Opts::new(
            "conduit_start_time_seconds",
            "Process start time as Unix timestamp",
        ))
        .expect("metric");
        let start_unix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs_f64())
            .unwrap_or(0.0);
        start_time_seconds.set(start_unix);

        let config_generation = Gauge::with_opts(Opts::new(
            "conduit_config_generation",
            "Active configuration generation",
        ))
        .expect("metric");

        registry
            .register(Box::new(queries_by_pool_total.clone()))
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
        registry
            .register(Box::new(forward_outstanding.clone()))
            .expect("register");
        registry
            .register(Box::new(pool_backends_configured.clone()))
            .expect("register");
        registry
            .register(Box::new(build_info.clone()))
            .expect("register");
        registry
            .register(Box::new(start_time_seconds.clone()))
            .expect("register");
        registry
            .register(Box::new(config_generation.clone()))
            .expect("register");

        let (process_resident_bytes, process_open_fds) = if effective && is_full {
            let rss = Gauge::with_opts(Opts::new(
                "conduit_process_resident_bytes",
                "Process resident set size in bytes",
            ))
            .expect("metric");
            let fds = Gauge::with_opts(Opts::new(
                "conduit_process_open_fds",
                "Open file descriptors",
            ))
            .expect("metric");
            registry.register(Box::new(rss.clone())).expect("register");
            registry.register(Box::new(fds.clone())).expect("register");
            (Some(rss), Some(fds))
        } else {
            (None, None)
        };

        Self {
            enabled: effective,
            profile,
            registry,
            queries_total,
            responses_total,
            parse_rejected_total,
            queries_by_pool_total,
            phase_duration,
            forward_attempts,
            forward_errors,
            forward_duration,
            retries_total,
            forward_outstanding,
            pool_backends_configured,
            build_info,
            start_time_seconds,
            config_generation,
            process_resident_bytes,
            process_open_fds,
            scrape_fn: RwLock::new(None),
        }
    }

    pub fn set_scrape_snapshot_fn(&self, f: ScrapeSnapshotFn) {
        *self.scrape_fn.write() = Some(f);
    }

    pub fn enabled(&self) -> bool {
        self.enabled
    }

    pub fn profile(&self) -> BuiltinProfile {
        self.profile
    }

    pub fn record_query(
        &self,
        listener: &str,
        protocol: &str,
        qtype: Option<u16>,
        qclass: Option<u16>,
        client_addr: &std::net::SocketAddr,
    ) {
        if !self.enabled {
            return;
        }
        match &self.queries_total {
            QueriesTotal::Minimal(v) => {
                v.with_label_values(&[listener, protocol]).inc();
            }
            QueriesTotal::Full(v) => {
                let qtype = qtype.map(qtype_label).unwrap_or_else(|| "UNKNOWN".into());
                let qclass = qclass.map(qclass_label).unwrap_or_else(|| "UNKNOWN".into());
                let ip_family = ip_family_label(client_addr);
                v.with_label_values(&[listener, protocol, &qtype, &qclass, ip_family])
                    .inc();
            }
        }
    }

    pub fn record_parse_rejected(&self, reason: &str) {
        if !self.enabled {
            return;
        }
        if let Some(ref c) = self.parse_rejected_total {
            c.with_label_values(&[reason]).inc();
        }
    }

    pub fn record_query_by_pool(&self, pool: &str) {
        if !self.enabled {
            return;
        }
        self.queries_by_pool_total.with_label_values(&[pool]).inc();
    }

    pub fn record_response(
        &self,
        listener: &str,
        protocol: &str,
        rcode: Option<u16>,
        client_addr: &std::net::SocketAddr,
    ) {
        if !self.enabled {
            return;
        }
        if let Some(ref responses) = self.responses_total {
            match responses {
                ResponsesTotal::Minimal(c) => {
                    let rcode = rcode_class_label(rcode);
                    c.with_label_values(&[listener, protocol, rcode]).inc();
                }
                ResponsesTotal::Full(c) => {
                    let rcode = rcode_label(rcode);
                    let ip_family = ip_family_label(client_addr);
                    c.with_label_values(&[listener, protocol, rcode, ip_family])
                        .inc();
                }
            }
        }
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

    fn refresh_scrape_gauges(&self) {
        let snapshot = self
            .scrape_fn
            .read()
            .as_ref()
            .map(|f| f())
            .unwrap_or_default();

        self.config_generation
            .set(snapshot.config_generation as f64);

        self.forward_outstanding.reset();
        for (pool, backend, count) in &snapshot.forward_outstanding {
            self.forward_outstanding
                .with_label_values(&[pool.as_str(), backend.as_str()])
                .set(*count as f64);
        }

        self.pool_backends_configured.reset();
        for (pool, count) in &snapshot.pool_backend_counts {
            self.pool_backends_configured
                .with_label_values(&[pool.as_str()])
                .set(*count as f64);
        }

        if self.profile == BuiltinProfile::Full {
            if let Some(ref rss) = self.process_resident_bytes {
                rss.set(read_resident_bytes().unwrap_or(0) as f64);
            }
            if let Some(ref fds) = self.process_open_fds {
                fds.set(read_open_fds().unwrap_or(0) as f64);
            }
        }
    }

    pub fn gather(&self) -> Vec<prometheus::proto::MetricFamily> {
        if self.enabled {
            self.refresh_scrape_gauges();
        }
        self.registry.gather()
    }

    /// Label names used by built-in counters (cardinality guard tests).
    pub fn builtin_label_names() -> Vec<&'static str> {
        vec![
            "listener",
            "protocol",
            "qtype",
            "qclass",
            "ip_family",
            "pool",
            "backend",
            "outcome",
            "reason",
            "rcode",
            "phase",
        ]
    }
}

fn read_resident_bytes() -> Option<u64> {
    let status = std::fs::read_to_string("/proc/self/status").ok()?;
    for line in status.lines() {
        if let Some(kb) = line.strip_prefix("VmRSS:") {
            let kb: u64 = kb.trim().trim_end_matches(" kB").parse().ok()?;
            return Some(kb * 1024);
        }
    }
    None
}

fn read_open_fds() -> Option<u64> {
    std::fs::read_dir("/proc/self/fd")
        .ok()
        .map(|d| d.count() as u64)
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

    #[test]
    fn full_profile_queries_include_qtype_label() {
        let reg = BuiltinRegistry::new(true, BuiltinProfile::Full);
        let addr: std::net::SocketAddr = "127.0.0.1:15353".parse().unwrap();
        reg.record_query("ln", "udp", Some(1), Some(1), &addr);
        let body = encode_builtin(reg.gather());
        assert!(body.contains(r#"qtype="A""#), "body:\n{body}");
    }

    #[test]
    fn cardinality_guard_no_qname_or_client_labels() {
        for name in BuiltinRegistry::builtin_label_names() {
            assert_ne!(name, "qname");
            assert_ne!(name, "client_ip");
            assert!(!name.contains("client_addr"));
        }
    }

    #[test]
    fn parse_rejected_recorded_in_full_profile() {
        let reg = BuiltinRegistry::new(true, BuiltinProfile::Full);
        reg.record_parse_rejected("wire_error");
        let body = encode_builtin(reg.gather());
        assert!(body.contains("conduit_parse_rejected_total"));
        assert!(body.contains(r#"reason="wire_error""#));
    }

    #[test]
    fn scrape_gauges_from_snapshot_fn() {
        let reg = BuiltinRegistry::new(true, BuiltinProfile::Full);
        reg.set_scrape_snapshot_fn(Arc::new(|| ScrapeGaugeSnapshot {
            config_generation: 7,
            pool_backend_counts: vec![("default".into(), 2)],
            forward_outstanding: vec![
                ("default".into(), "127.0.0.1:15300".into(), 0),
                ("default".into(), "127.0.0.1:15301".into(), 3),
            ],
        }));
        let body = encode_builtin(reg.gather());
        assert!(body.contains("conduit_config_generation 7"));
        assert!(body.contains(r#"pool="default""#));
        assert!(
            body.contains("conduit_forward_outstanding"),
            "body:\n{body}"
        );
        assert!(
            body.contains(
                r#"conduit_forward_outstanding{backend="127.0.0.1:15300",pool="default"} 0"#
            ),
            "body:\n{body}"
        );
        assert!(
            body.contains(
                r#"conduit_forward_outstanding{backend="127.0.0.1:15301",pool="default"} 3"#
            ),
            "body:\n{body}"
        );
    }

    #[test]
    fn process_block_on_scrape() {
        let reg = BuiltinRegistry::new(true, BuiltinProfile::Full);
        let body = encode_builtin(reg.gather());
        assert!(body.contains("conduit_build_info"));
        assert!(body.contains("conduit_start_time_seconds"));
        assert!(body.contains("conduit_config_generation"));
    }

    #[test]
    fn minimal_profile_responses_use_coarse_rcode_buckets() {
        let reg = BuiltinRegistry::new(true, BuiltinProfile::Minimal);
        let addr: std::net::SocketAddr = "127.0.0.1:15353".parse().unwrap();
        reg.record_response("ln", "udp", Some(9), &addr);
        reg.record_response("ln", "udp", Some(0), &addr);
        let body = encode_builtin(reg.gather());
        assert!(body.contains("conduit_responses_total"), "body:\n{body}");
        assert!(
            body.contains(r#"rcode="OTHER""#),
            "NOTAUTH should bucket to OTHER on minimal, body:\n{body}"
        );
        assert!(body.contains(r#"rcode="NOERROR""#), "body:\n{body}");
        assert!(
            !body.contains("ip_family="),
            "minimal responses omit ip_family, body:\n{body}"
        );
    }

    #[test]
    fn full_profile_responses_use_per_rcode_labels() {
        let reg = BuiltinRegistry::new(true, BuiltinProfile::Full);
        let addr: std::net::SocketAddr = "127.0.0.1:15353".parse().unwrap();
        reg.record_response("ln", "udp", Some(9), &addr);
        let body = encode_builtin(reg.gather());
        assert!(
            body.contains(r#"rcode="NOTAUTH""#),
            "full profile should expose NOTAUTH, body:\n{body}"
        );
        assert!(
            body.contains(r#"ip_family="v4""#),
            "full responses include ip_family, body:\n{body}"
        );
        assert!(
            !body.contains(r#"rcode_class="#),
            "label renamed to rcode, body:\n{body}"
        );
    }

    #[test]
    fn build_info_includes_compile_time_metadata_labels() {
        let reg = BuiltinRegistry::new(true, BuiltinProfile::Full);
        let body = encode_builtin(reg.gather());
        for (name, value) in crate::build_metadata::label_pairs() {
            assert!(
                body.contains(&format!(r#"{name}="{value}""#)),
                "missing label {name}={value} in:\n{body}"
            );
        }
    }
}
