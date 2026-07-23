//! Built-in Prometheus metrics (design §7.1).

use crate::compile::BuiltinProfile;
use crate::labels::{ip_family_label, qclass_label, qtype_label, rcode_class_label, rcode_label};
use parking_lot::RwLock;
use prometheus::{
    Encoder, Gauge, GaugeVec, HistogramOpts, HistogramVec, IntCounter, IntCounterVec, IntGauge,
    Opts, Registry, TextEncoder,
};
use std::sync::atomic::{AtomicU64, Ordering};
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
    pub slots_in_use: u32,
    pub slots_capacity: u32,
    pub slot_pool_exhausted_total: u64,
    /// Listener identity rows: `(label, address, name)`. `label` is the value used
    /// on traffic metrics (`listener` label = name-when-set, else address).
    pub listeners: Vec<ListenerIdentity>,
    /// Backend identity rows: `(pool, label, address, name)`. `label` is the value
    /// used on forward metrics (`backend` label = name-when-set, else address).
    pub backends: Vec<BackendIdentity>,
    /// Per-backend health scrape rows (empty when health is disabled).
    pub health_backends: Vec<HealthScrapeBackend>,
    /// Active (eligible) backend count per health-enabled pool.
    pub pool_backends_active: Vec<(String, u32)>,
    /// Approximate live cache entry count per named instance (full profile gauge).
    pub cache_entry_counts: Vec<(String, u64)>,
}

/// One backend row for health Prometheus series (phase 1c §10).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct HealthScrapeBackend {
    pub pool: String,
    pub backend: String,
    pub observed: f64,
    pub applied: f64,
    /// `1.0` when probe-driven transitions apply; `0.0` when frozen.
    pub probe_automatic: f64,
    pub effective_weight: f64,
    pub latency_ewma_ms: Option<f64>,
    pub transitions_total: u64,
}

/// One listener's identity + resolved ingress settings.
///
/// Drives `conduit_listener_info` (descriptive labels) plus the numeric
/// `conduit_listener_ingress_threads` / `conduit_listener_rcvbuf_bytes` gauges.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ListenerIdentity {
    /// Join key: matches the `listener` label on traffic metrics.
    pub label: String,
    /// Bind address (always present).
    pub address: String,
    /// Configured name, or empty when unnamed.
    pub name: String,
    /// `udp` / `tcp`.
    pub protocol: String,
    /// `v4` / `v6` (derived from the bind address).
    pub ip_family: String,
    /// Resolved `reuse_port` setting.
    pub reuse_port: bool,
    /// Resolved ingress worker thread count.
    pub threads: u32,
    /// Resolved socket receive buffer size in bytes (0 = OS default).
    pub rcvbuf: u32,
}

/// One backend's identity + resolved settings.
///
/// Drives `conduit_backend_info` (descriptive labels) plus the numeric
/// `conduit_backend_weight` gauge.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BackendIdentity {
    pub pool: String,
    /// Join key: matches the `backend` label on forward metrics.
    pub label: String,
    /// Backend `ip:port` address (always present).
    pub address: String,
    /// Configured name, or empty when unnamed.
    pub name: String,
    /// Effective load-balancing weight.
    pub weight: u32,
}

pub type ScrapeSnapshotFn = Arc<dyn Fn() -> ScrapeGaugeSnapshot + Send + Sync>;

enum QueriesTotal {
    Vec(IntCounterVec),
}

enum ResponsesTotal {
    Vec(IntCounterVec),
}

enum ResponsesTruncatedTotal {
    Minimal(IntCounterVec),
    Full(IntCounterVec),
}

enum QueriesDroppedTotal {
    Minimal(IntCounterVec),
    Full(IntCounterVec),
}

enum AclDecisionsTotal {
    Vec(IntCounterVec),
}

enum ResponseDuration {
    Full(HistogramVec),
}

/// Resolved label schemas for families whose Prometheus dimensions vary with
/// `metrics.granularity` (design §Decisions 5).
#[derive(Debug, Clone)]
struct FamilyLabelSchemas {
    volume: Vec<String>,
    responses: Vec<String>,
    responses_rcode: crate::plan::ResponsesRcodeBucketing,
    timing: Vec<String>,
    forward_failures: Vec<String>,
    acl: Vec<String>,
}

impl FamilyLabelSchemas {
    fn from_plan(plan: &crate::plan::CompiledMetricsPlan) -> Self {
        Self {
            volume: plan.dimensions_for("volume").to_vec(),
            responses: plan.dimensions_for("responses").to_vec(),
            responses_rcode: plan.responses_rcode,
            timing: plan.dimensions_for("timing").to_vec(),
            forward_failures: plan.dimensions_for("forward_failures").to_vec(),
            acl: plan.dimensions_for("acl").to_vec(),
        }
    }

    fn for_legacy_profile(profile: BuiltinProfile) -> Self {
        use crate::plan::{
            default_responses_rcode, preset_family_dimensions, Granularity, ResponsesRcodeBucketing,
        };
        let g = match profile {
            BuiltinProfile::Full => Granularity::Fine,
            BuiltinProfile::Minimal | BuiltinProfile::Off => Granularity::Coarse,
        };
        let dims = |family: &str| -> Vec<String> {
            preset_family_dimensions(family, g)
                .unwrap_or(&[])
                .iter()
                .map(|s| (*s).to_string())
                .collect()
        };
        Self {
            volume: dims("volume"),
            responses: dims("responses"),
            responses_rcode: match profile {
                BuiltinProfile::Full => ResponsesRcodeBucketing::Iana,
                _ => default_responses_rcode(g),
            },
            timing: dims("timing"),
            forward_failures: dims("forward_failures"),
            acl: dims("acl"),
        }
    }

    fn volume_has_ip_family(&self) -> bool {
        self.volume.iter().any(|d| d == "ip_family")
    }
}

fn label_refs(dims: &[String]) -> Vec<&str> {
    dims.iter().map(String::as_str).collect()
}

pub struct BuiltinRegistry {
    enabled: bool,
    profile: BuiltinProfile,
    /// `health` category membership from the compiled plan (metrics-configurability
    /// design §12). Independent of `profile`/granularity: `base: minimal` now
    /// includes health series — an intentional 1.x expansion vs the legacy
    /// `profile: minimal`, which omitted health entirely.
    health_enabled: bool,
    /// Plan collect mask for a representative set of categories (design
    /// §Decisions 4). Wired for `volume`, `failures`, and `timing` per the
    /// metrics-configurability G1 scope; `lookup`/`cache_detail` remain tied
    /// to `enabled`/`profile` for now (deferred to the granularity/export
    /// phases).
    collect_volume: bool,
    collect_failures: bool,
    collect_timing: bool,
    schemas: FamilyLabelSchemas,
    registry: Registry,
    queries_total: QueriesTotal,
    responses_total: Option<ResponsesTotal>,
    responses_truncated_total: Option<ResponsesTruncatedTotal>,
    queries_dropped_total: Option<QueriesDroppedTotal>,
    acl_decisions_total: Option<AclDecisionsTotal>,
    response_duration: Option<ResponseDuration>,
    lookup_provider_outcomes: Option<IntCounterVec>,
    cache_lookups: Option<IntCounterVec>,
    cache_fills: Option<IntCounterVec>,
    cache_singleflight_coalesced: Option<IntCounterVec>,
    lookup_duration: Option<HistogramVec>,
    cache_lookup_duration: Option<HistogramVec>,
    cache_evictions: Option<IntCounterVec>,
    cache_entries: Option<GaugeVec>,
    parse_rejected_total: Option<IntCounterVec>,
    queries_by_pool_total: IntCounterVec,
    phase_duration: HistogramVec,
    forward_attempts: IntCounterVec,
    forward_errors: IntCounterVec,
    forward_duration: HistogramVec,
    /// Active health-probe outcomes (`full` only).
    probe_results: Option<IntCounterVec>,
    retries_total: IntCounterVec,
    script_errors_total: Option<IntCounterVec>,
    forward_outstanding: GaugeVec,
    pool_backends_configured: GaugeVec,
    listener_info: GaugeVec,
    listener_ingress_threads: GaugeVec,
    listener_rcvbuf_bytes: GaugeVec,
    backend_info: GaugeVec,
    backend_weight: GaugeVec,
    backend_health_observed: Option<GaugeVec>,
    backend_health_applied: Option<GaugeVec>,
    backend_health_probe_automatic: Option<GaugeVec>,
    backend_health_effective_weight: Option<GaugeVec>,
    backend_health_latency_ewma_ms: Option<GaugeVec>,
    backend_health_transitions_total: Option<IntCounterVec>,
    pool_backends_active: Option<GaugeVec>,
    last_health_transitions_synced: RwLock<std::collections::HashMap<(String, String), u64>>,
    slots_in_use: Option<Gauge>,
    slots_capacity: Option<Gauge>,
    slot_pool_exhausted_total: Option<IntCounter>,
    last_exhaustion_synced: AtomicU64,
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
    /// Legacy two-tier constructor, preserved for existing call sites and
    /// tests. `health` registration is tied to `profile == Full`, matching
    /// today's shipped behavior exactly.
    pub fn new(enabled: bool, profile: BuiltinProfile) -> Self {
        let health_enabled = enabled && profile == BuiltinProfile::Full;
        // Legacy `minimal` omits timing hot-path recording (today's profile split).
        let collect_timing = profile == BuiltinProfile::Full;
        let schemas = FamilyLabelSchemas::for_legacy_profile(profile);
        Self::new_internal(
            enabled,
            profile,
            health_enabled,
            true,
            true,
            collect_timing,
            schemas,
        )
    }

    /// Plan-driven constructor (metrics-configurability design). Label schemas
    /// come from resolved `granularity` presets/overrides; `health` follows
    /// category collect (design §12 intentional minimal expansion).
    pub fn new_from_plan(plan: &crate::plan::CompiledMetricsPlan) -> Self {
        use crate::plan::{Granularity, MetricCategory};

        let enabled = plan.enabled;
        let profile = if !enabled {
            BuiltinProfile::Off
        } else if plan.granularity_default == Granularity::Fine {
            BuiltinProfile::Full
        } else {
            BuiltinProfile::Minimal
        };
        let health_enabled = plan.collect_for(MetricCategory::Health);
        let collect_volume = plan.collect_for(MetricCategory::Volume);
        let collect_failures = plan.collect_for(MetricCategory::Failures);
        let collect_timing = plan.collect_for(MetricCategory::Timing);
        let schemas = FamilyLabelSchemas::from_plan(plan);
        Self::new_internal(
            enabled,
            profile,
            health_enabled,
            collect_volume,
            collect_failures,
            collect_timing,
            schemas,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn new_internal(
        enabled: bool,
        profile: BuiltinProfile,
        health_enabled: bool,
        collect_volume: bool,
        collect_failures: bool,
        collect_timing: bool,
        schemas: FamilyLabelSchemas,
    ) -> Self {
        let registry = Registry::new();
        let effective = enabled && profile != BuiltinProfile::Off;
        let is_full = profile == BuiltinProfile::Full;
        let volume_labels = label_refs(&schemas.volume);
        let responses_labels = label_refs(&schemas.responses);
        let acl_labels = label_refs(&schemas.acl);
        let forward_failure_labels = label_refs(&schemas.forward_failures);

        // Timing attempts always include structural `outcome`; duration uses
        // timing dims only (`pool` / `backend` / empty aggregate).
        let mut forward_attempt_dims = schemas.timing.clone();
        forward_attempt_dims.push("outcome".to_string());
        let forward_attempt_labels = label_refs(&forward_attempt_dims);
        let forward_duration_labels = label_refs(&schemas.timing);

        let queries_total = {
            let v = IntCounterVec::new(
                Opts::new("conduit_queries_total", "DNS queries received"),
                &volume_labels,
            )
            .expect("metric");
            registry.register(Box::new(v.clone())).expect("register");
            QueriesTotal::Vec(v)
        };

        let responses_total = if effective {
            let v = IntCounterVec::new(
                Opts::new("conduit_responses_total", "DNS responses sent to clients"),
                &responses_labels,
            )
            .expect("metric");
            registry.register(Box::new(v.clone())).expect("register");
            Some(ResponsesTotal::Vec(v))
        } else {
            None
        };

        let responses_truncated_total = if effective {
            if schemas.volume_has_ip_family() {
                let v = IntCounterVec::new(
                    Opts::new(
                        "conduit_responses_truncated_total",
                        "UDP responses clipped to client payload size with TC set on send",
                    ),
                    &["listener", "protocol", "ip_family", "answer_source"],
                )
                .expect("metric");
                registry.register(Box::new(v.clone())).expect("register");
                Some(ResponsesTruncatedTotal::Full(v))
            } else {
                let v = IntCounterVec::new(
                    Opts::new(
                        "conduit_responses_truncated_total",
                        "UDP responses clipped to client payload size with TC set on send",
                    ),
                    &["listener", "protocol", "answer_source"],
                )
                .expect("metric");
                registry.register(Box::new(v.clone())).expect("register");
                Some(ResponsesTruncatedTotal::Minimal(v))
            }
        } else {
            None
        };

        let queries_dropped_total = if effective {
            if schemas.volume_has_ip_family() {
                let v = IntCounterVec::new(
                    Opts::new(
                        "conduit_queries_dropped_total",
                        "Queries ended with no DNS reply after successful parse (policy drop)",
                    ),
                    &["listener", "protocol", "reason", "ip_family"],
                )
                .expect("metric");
                registry.register(Box::new(v.clone())).expect("register");
                Some(QueriesDroppedTotal::Full(v))
            } else {
                let v = IntCounterVec::new(
                    Opts::new(
                        "conduit_queries_dropped_total",
                        "Queries ended with no DNS reply after successful parse (policy drop)",
                    ),
                    &["listener", "protocol", "reason"],
                )
                .expect("metric");
                registry.register(Box::new(v.clone())).expect("register");
                Some(QueriesDroppedTotal::Minimal(v))
            }
        } else {
            None
        };

        let acl_decisions_total = if effective {
            let v = IntCounterVec::new(
                Opts::new(
                    "conduit_acl_decisions_total",
                    "Client IP ACL decisions by tier and action",
                ),
                &acl_labels,
            )
            .expect("metric");
            registry.register(Box::new(v.clone())).expect("register");
            Some(AclDecisionsTotal::Vec(v))
        } else {
            None
        };

        let response_duration = if effective && is_full {
            let v = HistogramVec::new(
                HistogramOpts::new(
                    "conduit_response_duration_seconds",
                    "End-to-end client response time by answer source",
                )
                .buckets(phase_duration_buckets()),
                &["answer_source", "listener", "protocol"],
            )
            .expect("metric");
            registry.register(Box::new(v.clone())).expect("register");
            Some(ResponseDuration::Full(v))
        } else {
            None
        };

        let lookup_provider_outcomes = if effective {
            let v = IntCounterVec::new(
                Opts::new(
                    "conduit_lookup_provider_outcomes_total",
                    "Terminal lookup provider outcomes per attempt",
                ),
                &["profile", "provider", "outcome"],
            )
            .expect("metric");
            registry.register(Box::new(v.clone())).expect("register");
            Some(v)
        } else {
            None
        };

        let cache_lookups = if effective {
            let v = IntCounterVec::new(
                Opts::new("conduit_cache_lookups_total", "Cache read path results"),
                &["cache", "profile", "result"],
            )
            .expect("metric");
            registry.register(Box::new(v.clone())).expect("register");
            Some(v)
        } else {
            None
        };

        let (
            cache_fills,
            cache_singleflight_coalesced,
            lookup_duration,
            cache_lookup_duration,
            cache_evictions,
            cache_entries,
        ) = if effective && is_full {
            let fills = IntCounterVec::new(
                Opts::new(
                    "conduit_cache_fills_total",
                    "Successful cache stores after upstream answers",
                ),
                &["cache", "profile"],
            )
            .expect("metric");
            let coalesced = IntCounterVec::new(
                Opts::new(
                    "conduit_cache_singleflight_coalesced_total",
                    "Parallel identical cache misses answered from a shared in-progress fill",
                ),
                &["cache", "profile"],
            )
            .expect("metric");
            let lookup_dur = HistogramVec::new(
                HistogramOpts::new(
                    "conduit_lookup_duration_seconds",
                    "Wall time in lookup provider attempt",
                )
                .buckets(phase_duration_buckets()),
                &["profile", "provider"],
            )
            .expect("metric");
            let cache_dur = HistogramVec::new(
                HistogramOpts::new(
                    "conduit_cache_lookup_duration_seconds",
                    "Cache read path latency",
                )
                .buckets(phase_duration_buckets()),
                &["cache", "profile"],
            )
            .expect("metric");
            let evictions = IntCounterVec::new(
                Opts::new("conduit_cache_evictions_total", "Cache entry evictions"),
                &["cache", "reason"],
            )
            .expect("metric");
            let entries = GaugeVec::new(
                Opts::new(
                    "conduit_cache_entries",
                    "Approximate live cache entries per instance",
                ),
                &["cache"],
            )
            .expect("metric");
            for m in [&fills, &coalesced, &evictions] {
                registry.register(Box::new(m.clone())).expect("register");
            }
            for m in [&lookup_dur, &cache_dur] {
                registry.register(Box::new(m.clone())).expect("register");
            }
            registry
                .register(Box::new(entries.clone()))
                .expect("register");
            (
                Some(fills),
                Some(coalesced),
                Some(lookup_dur),
                Some(cache_dur),
                Some(evictions),
                Some(entries),
            )
        } else {
            (None, None, None, None, None, None)
        };

        let parse_rejected_total = if effective {
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
            &forward_attempt_labels,
        )
        .expect("metric");
        let forward_errors = IntCounterVec::new(
            Opts::new("conduit_forward_errors_total", "Forward errors"),
            &forward_failure_labels,
        )
        .expect("metric");
        let forward_duration = HistogramVec::new(
            HistogramOpts::new(
                "conduit_forward_duration_seconds",
                "Upstream forward round-trip time",
            )
            .buckets(forward_duration_buckets()),
            &forward_duration_labels,
        )
        .expect("metric");
        let retries_total = IntCounterVec::new(
            Opts::new("conduit_retries_total", "Retry transitions"),
            &["pool"],
        )
        .expect("metric");
        let script_errors_total = if effective {
            let v = IntCounterVec::new(
                Opts::new(
                    "conduit_script_errors_total",
                    "Rhai script errors and lookup faults",
                ),
                &["reason", "script", "table"],
            )
            .expect("metric");
            registry.register(Box::new(v.clone())).expect("register");
            Some(v)
        } else {
            None
        };
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
        let listener_info = GaugeVec::new(
            Opts::new(
                "conduit_listener_info",
                "Configured listener identity; join on `listener` (and `protocol`)",
            ),
            &[
                "listener",
                "address",
                "name",
                "protocol",
                "ip_family",
                "reuse_port",
            ],
        )
        .expect("metric");
        let listener_ingress_threads = GaugeVec::new(
            Opts::new(
                "conduit_listener_ingress_threads",
                "Resolved ingress worker threads per listener",
            ),
            &["listener", "protocol"],
        )
        .expect("metric");
        let listener_rcvbuf_bytes = GaugeVec::new(
            Opts::new(
                "conduit_listener_rcvbuf_bytes",
                "Resolved socket receive buffer per listener (0 = OS default)",
            ),
            &["listener", "protocol"],
        )
        .expect("metric");
        let backend_info = GaugeVec::new(
            Opts::new(
                "conduit_backend_info",
                "Configured backend identity; join on `pool` and the `backend` label",
            ),
            &["pool", "backend", "address", "name"],
        )
        .expect("metric");
        let backend_weight = GaugeVec::new(
            Opts::new(
                "conduit_backend_weight",
                "Effective load-balancing weight per backend",
            ),
            &["pool", "backend"],
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
            .register(Box::new(listener_info.clone()))
            .expect("register");
        registry
            .register(Box::new(listener_ingress_threads.clone()))
            .expect("register");
        registry
            .register(Box::new(listener_rcvbuf_bytes.clone()))
            .expect("register");
        registry
            .register(Box::new(backend_info.clone()))
            .expect("register");
        registry
            .register(Box::new(backend_weight.clone()))
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

        let slot_pool_exhausted_total = if effective {
            let v = IntCounter::with_opts(Opts::new(
                "conduit_slot_pool_exhausted_total",
                "Transaction slot pool acquire failures at capacity",
            ))
            .expect("metric");
            registry.register(Box::new(v.clone())).expect("register");
            Some(v)
        } else {
            None
        };

        let (slots_in_use, slots_capacity) = if is_full {
            let in_use = Gauge::with_opts(Opts::new(
                "conduit_slots_in_use",
                "Transaction slots currently in use",
            ))
            .expect("metric");
            let capacity = Gauge::with_opts(Opts::new(
                "conduit_slots_capacity",
                "Configured transaction slot pool capacity",
            ))
            .expect("metric");
            registry
                .register(Box::new(in_use.clone()))
                .expect("register");
            registry
                .register(Box::new(capacity.clone()))
                .expect("register");
            (Some(in_use), Some(capacity))
        } else {
            (None, None)
        };

        let (
            backend_health_observed,
            backend_health_applied,
            backend_health_probe_automatic,
            backend_health_effective_weight,
            backend_health_latency_ewma_ms,
            backend_health_transitions_total,
            pool_backends_active,
            probe_results,
        ) = if health_enabled {
            let observed = GaugeVec::new(
                Opts::new(
                    "conduit_backend_health_observed",
                    "Probe-derived health per backend (0=unknown, 1=up, 2=down)",
                ),
                &["pool", "backend"],
            )
            .expect("metric");
            let applied = GaugeVec::new(
                Opts::new(
                    "conduit_backend_health_applied",
                    "Health applied to routing per backend (0=unknown, 1=up, 2=down)",
                ),
                &["pool", "backend"],
            )
            .expect("metric");
            let probe_automatic = GaugeVec::new(
                Opts::new(
                    "conduit_backend_health_probe_automatic",
                    "Whether probe-driven transitions apply (1=automatic, 0=frozen)",
                ),
                &["pool", "backend"],
            )
            .expect("metric");
            let effective_weight = GaugeVec::new(
                Opts::new(
                    "conduit_backend_health_effective_weight",
                    "Effective load-balancing weight Route uses for this backend",
                ),
                &["pool", "backend"],
            )
            .expect("metric");
            let latency_ewma = GaugeVec::new(
                Opts::new(
                    "conduit_backend_health_latency_ewma_ms",
                    "Probe latency EWMA in milliseconds",
                ),
                &["pool", "backend"],
            )
            .expect("metric");
            let transitions = IntCounterVec::new(
                Opts::new(
                    "conduit_backend_health_transitions_total",
                    "Cumulative observed or applied health transitions per backend",
                ),
                &["pool", "backend"],
            )
            .expect("metric");
            let active = GaugeVec::new(
                Opts::new(
                    "conduit_pool_backends_active",
                    "Eligible backends per pool (applied health up)",
                ),
                &["pool"],
            )
            .expect("metric");
            let probe_results = IntCounterVec::new(
                Opts::new(
                    "conduit_probe_results_total",
                    "Active health-probe outcomes per backend",
                ),
                &["pool", "backend", "outcome"],
            )
            .expect("metric");
            for m in [
                &observed,
                &applied,
                &probe_automatic,
                &effective_weight,
                &latency_ewma,
                &active,
            ] {
                registry.register(Box::new(m.clone())).expect("register");
            }
            registry
                .register(Box::new(transitions.clone()))
                .expect("register");
            registry
                .register(Box::new(probe_results.clone()))
                .expect("register");
            (
                Some(observed),
                Some(applied),
                Some(probe_automatic),
                Some(effective_weight),
                Some(latency_ewma),
                Some(transitions),
                Some(active),
                Some(probe_results),
            )
        } else {
            (None, None, None, None, None, None, None, None)
        };

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
            health_enabled: effective && health_enabled,
            collect_volume,
            collect_failures,
            collect_timing,
            schemas,
            registry,
            queries_total,
            responses_total,
            responses_truncated_total,
            queries_dropped_total,
            acl_decisions_total,
            response_duration,
            lookup_provider_outcomes,
            cache_lookups,
            cache_fills,
            cache_singleflight_coalesced,
            lookup_duration,
            cache_lookup_duration,
            cache_evictions,
            cache_entries,
            parse_rejected_total,
            queries_by_pool_total,
            phase_duration,
            forward_attempts,
            forward_errors,
            forward_duration,
            probe_results,
            retries_total,
            script_errors_total,
            forward_outstanding,
            pool_backends_configured,
            listener_info,
            listener_ingress_threads,
            listener_rcvbuf_bytes,
            backend_info,
            backend_weight,
            backend_health_observed,
            backend_health_applied,
            backend_health_probe_automatic,
            backend_health_effective_weight,
            backend_health_latency_ewma_ms,
            backend_health_transitions_total,
            pool_backends_active,
            last_health_transitions_synced: RwLock::new(std::collections::HashMap::new()),
            slots_in_use,
            slots_capacity,
            slot_pool_exhausted_total,
            last_exhaustion_synced: AtomicU64::new(0),
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

    /// Whether the `health` category is active in the compiled plan (design
    /// §12). Independent of `profile`; see [`Self::new_from_plan`].
    pub fn health_enabled(&self) -> bool {
        self.health_enabled
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
        if !self.enabled || !self.collect_volume {
            return;
        }
        let QueriesTotal::Vec(v) = &self.queries_total;
        let qtype_s = qtype.map(qtype_label).unwrap_or_else(|| "UNKNOWN".into());
        let qclass_s = qclass.map(qclass_label).unwrap_or_else(|| "UNKNOWN".into());
        let ip_family = ip_family_label(client_addr);
        let mut vals: Vec<&str> = Vec::with_capacity(self.schemas.volume.len());
        for dim in &self.schemas.volume {
            match dim.as_str() {
                "listener" => vals.push(listener),
                "protocol" => vals.push(protocol),
                "qtype" => vals.push(&qtype_s),
                "qclass" => vals.push(&qclass_s),
                "ip_family" => vals.push(ip_family),
                _ => vals.push(""),
            }
        }
        v.with_label_values(&vals).inc();
    }

    pub fn record_parse_rejected(&self, reason: &str) {
        if !self.enabled || !self.collect_failures {
            return;
        }
        if let Some(ref c) = self.parse_rejected_total {
            c.with_label_values(&[reason]).inc();
        }
    }

    /// Policy drop after successful parse (`reason`: `request_rules` or `response_rules`).
    pub fn record_query_dropped(
        &self,
        listener: &str,
        protocol: &str,
        reason: &str,
        client_addr: &std::net::SocketAddr,
    ) {
        if !self.enabled || !self.collect_volume {
            return;
        }
        if let Some(ref dropped) = self.queries_dropped_total {
            match dropped {
                QueriesDroppedTotal::Minimal(c) => {
                    c.with_label_values(&[listener, protocol, reason]).inc();
                }
                QueriesDroppedTotal::Full(c) => {
                    let ip_family = ip_family_label(client_addr);
                    c.with_label_values(&[listener, protocol, reason, ip_family])
                        .inc();
                }
            }
        }
    }

    /// Record one client IP ACL decision.
    ///
    /// `tier` is `preadmission` (pre-parse drop-only gate) or `listener`
    /// (full first-match). `action` is `drop` | `refuse` | `tag` | `admit`.
    pub fn record_acl_decision(
        &self,
        tier: &str,
        action: &str,
        listener: &str,
        client_ip: std::net::IpAddr,
    ) {
        if !self.enabled || !self.collect_volume {
            return;
        }
        if let Some(AclDecisionsTotal::Vec(c)) = self.acl_decisions_total.as_ref() {
            let ip_family = if client_ip.is_ipv6() { "v6" } else { "v4" };
            let mut vals: Vec<&str> = Vec::with_capacity(self.schemas.acl.len());
            for dim in &self.schemas.acl {
                match dim.as_str() {
                    "tier" => vals.push(tier),
                    "action" => vals.push(action),
                    "listener" => vals.push(listener),
                    "ip_family" => vals.push(ip_family),
                    _ => vals.push(""),
                }
            }
            c.with_label_values(&vals).inc();
        }
    }

    pub fn record_query_by_pool(&self, pool: &str) {
        if !self.enabled || !self.collect_volume {
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
        answer_source: Option<&str>,
    ) {
        if !self.enabled || !self.collect_volume {
            return;
        }
        let answer_source = answer_source.unwrap_or("");
        let Some(ResponsesTotal::Vec(c)) = self.responses_total.as_ref() else {
            return;
        };
        use crate::plan::ResponsesRcodeBucketing;
        let rcode_s = match self.schemas.responses_rcode {
            ResponsesRcodeBucketing::Coarse => rcode_class_label(rcode).to_string(),
            ResponsesRcodeBucketing::Iana => rcode_label(rcode).to_string(),
        };
        let ip_family = ip_family_label(client_addr);
        let mut vals: Vec<&str> = Vec::with_capacity(self.schemas.responses.len());
        for dim in &self.schemas.responses {
            match dim.as_str() {
                "listener" => vals.push(listener),
                "protocol" => vals.push(protocol),
                "rcode" => vals.push(&rcode_s),
                "ip_family" => vals.push(ip_family),
                "answer_source" => vals.push(answer_source),
                _ => vals.push(""),
            }
        }
        c.with_label_values(&vals).inc();
    }

    pub fn record_response_truncated(
        &self,
        listener: &str,
        protocol: &str,
        client_addr: &std::net::SocketAddr,
        answer_source: Option<&str>,
    ) {
        if !self.enabled || !self.collect_volume {
            return;
        }
        let answer_source = answer_source.unwrap_or("");
        if let Some(ref responses) = self.responses_truncated_total {
            match responses {
                ResponsesTruncatedTotal::Minimal(c) => {
                    c.with_label_values(&[listener, protocol, answer_source])
                        .inc();
                }
                ResponsesTruncatedTotal::Full(c) => {
                    let ip_family = ip_family_label(client_addr);
                    c.with_label_values(&[listener, protocol, ip_family, answer_source])
                        .inc();
                }
            }
        }
    }

    pub fn observe_response_duration(
        &self,
        answer_source: Option<&str>,
        listener: &str,
        protocol: &str,
        duration_secs: f64,
    ) {
        if !self.enabled || self.profile != BuiltinProfile::Full {
            return;
        }
        if let Some(ResponseDuration::Full(h)) = self.response_duration.as_ref() {
            h.with_label_values(&[answer_source.unwrap_or(""), listener, protocol])
                .observe(duration_secs);
        }
    }

    pub fn record_lookup_provider_outcome(&self, profile: &str, provider: &str, outcome: &str) {
        if !self.enabled {
            return;
        }
        if let Some(c) = self.lookup_provider_outcomes.as_ref() {
            c.with_label_values(&[profile, provider, outcome]).inc();
        }
    }

    pub fn record_cache_lookup(&self, cache: &str, profile: &str, result: &str) {
        if !self.enabled {
            return;
        }
        if let Some(c) = self.cache_lookups.as_ref() {
            c.with_label_values(&[cache, profile, result]).inc();
        }
    }

    pub fn record_cache_fill(&self, cache: &str, profile: &str) {
        if !self.enabled || self.profile != BuiltinProfile::Full {
            return;
        }
        if let Some(c) = self.cache_fills.as_ref() {
            c.with_label_values(&[cache, profile]).inc();
        }
    }

    pub fn record_cache_singleflight_coalesced(&self, cache: &str, profile: &str) {
        if !self.enabled || self.profile != BuiltinProfile::Full {
            return;
        }
        if let Some(c) = self.cache_singleflight_coalesced.as_ref() {
            c.with_label_values(&[cache, profile]).inc();
        }
    }

    pub fn observe_lookup_duration(&self, profile: &str, provider: &str, duration_secs: f64) {
        if !self.enabled || self.profile != BuiltinProfile::Full {
            return;
        }
        if let Some(h) = self.lookup_duration.as_ref() {
            h.with_label_values(&[profile, provider])
                .observe(duration_secs);
        }
    }

    pub fn observe_cache_lookup_duration(&self, cache: &str, profile: &str, duration_secs: f64) {
        if !self.enabled || self.profile != BuiltinProfile::Full {
            return;
        }
        if let Some(h) = self.cache_lookup_duration.as_ref() {
            h.with_label_values(&[cache, profile])
                .observe(duration_secs);
        }
    }

    pub fn record_cache_eviction(&self, cache: &str, reason: &str) {
        self.record_cache_evictions(cache, reason, 1);
    }

    pub fn record_cache_evictions(&self, cache: &str, reason: &str, count: u64) {
        if count == 0 || !self.enabled || self.profile != BuiltinProfile::Full {
            return;
        }
        if let Some(c) = self.cache_evictions.as_ref() {
            c.with_label_values(&[cache, reason]).inc_by(count);
        }
    }

    pub fn observe_phase(&self, phase: &str, duration_secs: f64) {
        if !self.enabled || !self.collect_timing {
            return;
        }
        self.phase_duration
            .with_label_values(&[phase])
            .observe(duration_secs);
    }

    pub fn record_forward_attempt(&self, pool: &str, backend: &str, outcome: &str) {
        if !self.enabled || !self.collect_timing {
            return;
        }
        let mut vals: Vec<&str> = Vec::with_capacity(self.schemas.timing.len() + 1);
        for dim in &self.schemas.timing {
            match dim.as_str() {
                "pool" => vals.push(pool),
                "backend" => vals.push(backend),
                _ => vals.push(""),
            }
        }
        vals.push(outcome);
        self.forward_attempts.with_label_values(&vals).inc();
    }

    pub fn record_forward_error(&self, pool: &str, backend: &str, reason: &str) {
        if !self.enabled || !self.collect_failures {
            return;
        }
        let mut vals: Vec<&str> = Vec::with_capacity(self.schemas.forward_failures.len());
        for dim in &self.schemas.forward_failures {
            match dim.as_str() {
                "pool" => vals.push(pool),
                "backend" => vals.push(backend),
                "reason" => vals.push(reason),
                _ => vals.push(""),
            }
        }
        self.forward_errors.with_label_values(&vals).inc();
    }

    /// Record one active health-probe outcome (`full` profile only).
    ///
    /// `outcome` is one of: `success`, `failure` (unacceptable reply),
    /// `timeout`, or `send_error` (transport send/connect failure).
    pub fn record_probe_result(&self, pool: &str, backend: &str, outcome: &str) {
        if !self.enabled || !self.health_enabled {
            return;
        }
        let Some(counter) = self.probe_results.as_ref() else {
            return;
        };
        counter.with_label_values(&[pool, backend, outcome]).inc();
    }

    pub fn record_forward_duration(&self, pool: &str, backend: &str, duration_secs: f64) {
        if !self.enabled || !self.collect_timing {
            return;
        }
        let mut vals: Vec<&str> = Vec::with_capacity(self.schemas.timing.len());
        for dim in &self.schemas.timing {
            match dim.as_str() {
                "pool" => vals.push(pool),
                "backend" => vals.push(backend),
                _ => vals.push(""),
            }
        }
        self.forward_duration
            .with_label_values(&vals)
            .observe(duration_secs);
    }

    pub fn record_retry(&self, pool: &str) {
        if !self.enabled || !self.collect_failures {
            return;
        }
        self.retries_total.with_label_values(&[pool]).inc();
    }

    pub fn record_script_error(&self, reason: &str, script: &str, table: &str) {
        if !self.enabled || !self.collect_failures {
            return;
        }
        if let Some(ref c) = self.script_errors_total {
            c.with_label_values(&[reason, script, table]).inc();
        }
    }

    fn sync_exhaustion_counter(&self, total: u64) {
        let Some(counter) = self.slot_pool_exhausted_total.as_ref() else {
            return;
        };
        let prev = self.last_exhaustion_synced.load(Ordering::Relaxed);
        if total > prev {
            counter.inc_by(total - prev);
            self.last_exhaustion_synced.store(total, Ordering::Relaxed);
        }
    }

    fn sync_health_transition_counters(&self, rows: &[HealthScrapeBackend]) {
        let Some(counter) = self.backend_health_transitions_total.as_ref() else {
            return;
        };
        let mut last = self.last_health_transitions_synced.write();
        let mut seen = std::collections::HashSet::new();
        for row in rows {
            let key = (row.pool.clone(), row.backend.clone());
            seen.insert(key.clone());
            let prev = last.get(&key).copied().unwrap_or(0);
            if row.transitions_total > prev {
                counter
                    .with_label_values(&[row.pool.as_str(), row.backend.as_str()])
                    .inc_by(row.transitions_total - prev);
                last.insert(key, row.transitions_total);
            }
        }
        last.retain(|k, _| seen.contains(k));
    }

    fn refresh_health_scrape_gauges(&self, snapshot: &ScrapeGaugeSnapshot) {
        let Some(observed) = self.backend_health_observed.as_ref() else {
            return;
        };
        observed.reset();
        self.backend_health_applied
            .as_ref()
            .expect("health pair")
            .reset();
        self.backend_health_probe_automatic
            .as_ref()
            .expect("health pair")
            .reset();
        self.backend_health_effective_weight
            .as_ref()
            .expect("health pair")
            .reset();
        self.backend_health_latency_ewma_ms
            .as_ref()
            .expect("health pair")
            .reset();
        for row in &snapshot.health_backends {
            let labels = [row.pool.as_str(), row.backend.as_str()];
            observed.with_label_values(&labels).set(row.observed);
            self.backend_health_applied
                .as_ref()
                .expect("health pair")
                .with_label_values(&labels)
                .set(row.applied);
            self.backend_health_probe_automatic
                .as_ref()
                .expect("health pair")
                .with_label_values(&labels)
                .set(row.probe_automatic);
            self.backend_health_effective_weight
                .as_ref()
                .expect("health pair")
                .with_label_values(&labels)
                .set(row.effective_weight);
            if let Some(ms) = row.latency_ewma_ms {
                self.backend_health_latency_ewma_ms
                    .as_ref()
                    .expect("health pair")
                    .with_label_values(&labels)
                    .set(ms);
            }
        }
        self.sync_health_transition_counters(&snapshot.health_backends);

        if let Some(active) = self.pool_backends_active.as_ref() {
            active.reset();
            for (pool, count) in &snapshot.pool_backends_active {
                active
                    .with_label_values(&[pool.as_str()])
                    .set(*count as f64);
            }
        }
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

        self.listener_info.reset();
        self.listener_ingress_threads.reset();
        self.listener_rcvbuf_bytes.reset();
        for l in &snapshot.listeners {
            let reuse_port = if l.reuse_port { "true" } else { "false" };
            self.listener_info
                .with_label_values(&[
                    l.label.as_str(),
                    l.address.as_str(),
                    l.name.as_str(),
                    l.protocol.as_str(),
                    l.ip_family.as_str(),
                    reuse_port,
                ])
                .set(1.0);
            self.listener_ingress_threads
                .with_label_values(&[l.label.as_str(), l.protocol.as_str()])
                .set(l.threads as f64);
            self.listener_rcvbuf_bytes
                .with_label_values(&[l.label.as_str(), l.protocol.as_str()])
                .set(l.rcvbuf as f64);
        }

        self.backend_info.reset();
        self.backend_weight.reset();
        for b in &snapshot.backends {
            self.backend_info
                .with_label_values(&[
                    b.pool.as_str(),
                    b.label.as_str(),
                    b.address.as_str(),
                    b.name.as_str(),
                ])
                .set(1.0);
            self.backend_weight
                .with_label_values(&[b.pool.as_str(), b.label.as_str()])
                .set(b.weight as f64);
        }

        if let Some(g) = self.slots_in_use.as_ref() {
            g.set(snapshot.slots_in_use as f64);
        }
        if let Some(g) = self.slots_capacity.as_ref() {
            g.set(snapshot.slots_capacity as f64);
        }
        self.sync_exhaustion_counter(snapshot.slot_pool_exhausted_total);

        self.refresh_health_scrape_gauges(&snapshot);

        if let Some(gauge) = self.cache_entries.as_ref() {
            gauge.reset();
            for (cache, count) in &snapshot.cache_entry_counts {
                gauge
                    .with_label_values(&[cache.as_str()])
                    .set(*count as f64);
            }
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
            "profile",
            "provider",
            "cache",
            "result",
            "answer_source",
            "tier",
            "action",
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
    fn failure_counters_recorded_on_minimal_and_full() {
        for profile in [BuiltinProfile::Minimal, BuiltinProfile::Full] {
            let reg = BuiltinRegistry::new(true, profile);
            let addr: std::net::SocketAddr = "127.0.0.1:15353".parse().unwrap();
            reg.record_script_error(
                crate::SCRIPT_ERROR_LOOKUP_UNKNOWN_TABLE,
                "scripts/blocklist.rhai",
                "typo_table",
            );
            reg.record_parse_rejected("wire_error");
            reg.record_query_dropped("ln", "udp", "request_rules", &addr);
            reg.record_forward_error("default", "127.0.0.1:5300", "timeout");
            reg.record_retry("default");
            let body = encode_builtin(reg.gather());
            assert!(
                body.contains("conduit_script_errors_total"),
                "{profile:?} body:\n{body}"
            );
            assert!(
                body.contains(r#"reason="lookup_unknown_table""#),
                "{profile:?} body:\n{body}"
            );
            assert!(
                body.contains("conduit_parse_rejected_total"),
                "{profile:?} body:\n{body}"
            );
            assert!(
                body.contains("conduit_queries_dropped_total"),
                "{profile:?} body:\n{body}"
            );
            assert!(
                body.contains(r#"reason="request_rules""#),
                "{profile:?} body:\n{body}"
            );
            assert!(
                body.contains(
                    r#"conduit_forward_errors_total{backend="127.0.0.1:5300",pool="default",reason="timeout"}"#
                ),
                "{profile:?} body:\n{body}"
            );
            assert!(
                body.contains("conduit_retries_total"),
                "{profile:?} body:\n{body}"
            );
        }
    }

    #[test]
    fn probe_results_recorded_only_on_full() {
        let minimal = BuiltinRegistry::new(true, BuiltinProfile::Minimal);
        minimal.record_probe_result("default", "127.0.0.1:5300", "timeout");
        let body_min = encode_builtin(minimal.gather());
        assert!(
            !body_min.contains("conduit_probe_results_total"),
            "minimal must not record probe results, body:\n{body_min}"
        );

        let full = BuiltinRegistry::new(true, BuiltinProfile::Full);
        full.record_probe_result("default", "127.0.0.1:5300", "success");
        full.record_probe_result("default", "127.0.0.1:5300", "timeout");
        full.record_probe_result("default", "127.0.0.1:5300", "failure");
        full.record_probe_result("default", "127.0.0.1:5300", "send_error");
        let body_full = encode_builtin(full.gather());
        for outcome in ["success", "timeout", "failure", "send_error"] {
            assert!(
                body_full.contains(&format!(
                    r#"conduit_probe_results_total{{backend="127.0.0.1:5300",outcome="{outcome}",pool="default"}}"#
                )),
                "missing outcome={outcome} in:\n{body_full}"
            );
        }
    }

    #[test]
    fn forward_attempts_recorded_only_on_full() {
        let minimal = BuiltinRegistry::new(true, BuiltinProfile::Minimal);
        minimal.record_forward_attempt("default", "127.0.0.1:5300", "success");
        let body_min = encode_builtin(minimal.gather());
        assert!(
            !body_min.contains("conduit_forward_attempts_total"),
            "minimal must not record forward attempts, body:\n{body_min}"
        );

        let full = BuiltinRegistry::new(true, BuiltinProfile::Full);
        full.record_forward_attempt("default", "127.0.0.1:5300", "success");
        let body_full = encode_builtin(full.gather());
        assert!(
            body_full.contains("conduit_forward_attempts_total"),
            "body:\n{body_full}"
        );
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
                ("default".into(), "resolver-east".into(), 3),
            ],
            slots_in_use: 10,
            slots_capacity: 1024,
            slot_pool_exhausted_total: 2,
            listeners: vec![
                ListenerIdentity {
                    label: "lab-udp".into(),
                    address: "127.0.0.1:15353".into(),
                    name: "lab-udp".into(),
                    protocol: "udp".into(),
                    ip_family: "v4".into(),
                    reuse_port: true,
                    threads: 4,
                    rcvbuf: 1_048_576,
                },
                ListenerIdentity {
                    label: "127.0.0.1:15354".into(),
                    address: "127.0.0.1:15354".into(),
                    name: String::new(),
                    protocol: "tcp".into(),
                    ip_family: "v4".into(),
                    reuse_port: false,
                    threads: 1,
                    rcvbuf: 0,
                },
            ],
            backends: vec![
                BackendIdentity {
                    pool: "default".into(),
                    label: "resolver-east".into(),
                    address: "127.0.0.1:15300".into(),
                    name: "resolver-east".into(),
                    weight: 100,
                },
                BackendIdentity {
                    pool: "default".into(),
                    label: "127.0.0.1:15301".into(),
                    address: "127.0.0.1:15301".into(),
                    name: String::new(),
                    weight: 50,
                },
            ],
            health_backends: Vec::new(),
            pool_backends_active: Vec::new(),
            cache_entry_counts: Vec::new(),
        }));
        let body = encode_builtin(reg.gather());
        assert!(body.contains("conduit_config_generation 7"));
        assert!(
            body.contains(
                r#"conduit_listener_info{address="127.0.0.1:15353",ip_family="v4",listener="lab-udp",name="lab-udp",protocol="udp",reuse_port="true"} 1"#
            ),
            "body:\n{body}"
        );
        assert!(
            body.contains(
                r#"conduit_listener_info{address="127.0.0.1:15354",ip_family="v4",listener="127.0.0.1:15354",name="",protocol="tcp",reuse_port="false"} 1"#
            ),
            "body:\n{body}"
        );
        assert!(
            body.contains(
                r#"conduit_listener_ingress_threads{listener="lab-udp",protocol="udp"} 4"#
            ),
            "body:\n{body}"
        );
        assert!(
            body.contains(
                r#"conduit_listener_rcvbuf_bytes{listener="lab-udp",protocol="udp"} 1048576"#
            ),
            "body:\n{body}"
        );
        assert!(
            body.contains(
                r#"conduit_backend_info{address="127.0.0.1:15300",backend="resolver-east",name="resolver-east",pool="default"} 1"#
            ),
            "body:\n{body}"
        );
        assert!(
            body.contains(r#"conduit_backend_weight{backend="resolver-east",pool="default"} 100"#),
            "body:\n{body}"
        );
        assert!(
            body.contains(r#"conduit_backend_weight{backend="127.0.0.1:15301",pool="default"} 50"#),
            "body:\n{body}"
        );
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
                r#"conduit_forward_outstanding{backend="resolver-east",pool="default"} 3"#
            ),
            "body:\n{body}"
        );
        assert!(body.contains("conduit_slots_in_use 10"), "body:\n{body}");
        assert!(
            body.contains("conduit_slots_capacity 1024"),
            "body:\n{body}"
        );
        assert!(
            body.contains("conduit_slot_pool_exhausted_total 2"),
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
    fn responses_truncated_joinable_with_responses_labels() {
        let reg = BuiltinRegistry::new(true, BuiltinProfile::Minimal);
        let addr: std::net::SocketAddr = "127.0.0.1:15353".parse().unwrap();
        reg.record_response_truncated("ln", "udp", &addr, Some("cache"));
        reg.record_response_truncated("ln", "udp", &addr, Some("forward"));
        let body = encode_builtin(reg.gather());
        assert!(
            body.contains("conduit_responses_truncated_total"),
            "body:\n{body}"
        );
        assert!(body.contains(r#"answer_source="cache""#), "body:\n{body}");
        assert!(
            !body.contains("ip_family="),
            "minimal truncated responses omit ip_family, body:\n{body}"
        );

        let reg = BuiltinRegistry::new(true, BuiltinProfile::Full);
        reg.record_response_truncated("ln", "udp", &addr, Some("cache"));
        let body = encode_builtin(reg.gather());
        assert!(
            body.contains(r#"ip_family="v4""#),
            "full truncated responses include ip_family, body:\n{body}"
        );
    }

    #[test]
    fn queries_dropped_joinable_labels_and_reason() {
        let addr: std::net::SocketAddr = "127.0.0.1:15353".parse().unwrap();
        let reg = BuiltinRegistry::new(true, BuiltinProfile::Minimal);
        reg.record_query_dropped("ln", "udp", "request_rules", &addr);
        reg.record_query_dropped("ln", "udp", "response_rules", &addr);
        let body = encode_builtin(reg.gather());
        assert!(
            body.contains("conduit_queries_dropped_total"),
            "body:\n{body}"
        );
        assert!(body.contains(r#"reason="request_rules""#), "body:\n{body}");
        assert!(body.contains(r#"reason="response_rules""#), "body:\n{body}");
        assert!(
            !body.contains("ip_family="),
            "minimal policy drops omit ip_family, body:\n{body}"
        );

        let reg = BuiltinRegistry::new(true, BuiltinProfile::Full);
        reg.record_query_dropped("ln", "udp", "request_rules", &addr);
        let body = encode_builtin(reg.gather());
        assert!(
            body.contains(r#"ip_family="v4""#),
            "full policy drops include ip_family, body:\n{body}"
        );
        assert!(body.contains(r#"reason="request_rules""#), "body:\n{body}");
    }

    #[test]
    fn acl_decisions_recorded_on_minimal_and_full() {
        let v4: std::net::IpAddr = "10.1.2.3".parse().unwrap();
        let v6: std::net::IpAddr = "2001:db8::1".parse().unwrap();

        let reg = BuiltinRegistry::new(true, BuiltinProfile::Minimal);
        reg.record_acl_decision("preadmission", "drop", "ln", v4);
        reg.record_acl_decision("listener", "refuse", "ln", v4);
        let body = encode_builtin(reg.gather());
        assert!(
            body.contains("conduit_acl_decisions_total"),
            "body:\n{body}"
        );
        assert!(body.contains(r#"tier="preadmission""#), "body:\n{body}");
        assert!(body.contains(r#"action="refuse""#), "body:\n{body}");
        assert!(
            !body.contains("ip_family="),
            "minimal acl decisions omit ip_family, body:\n{body}"
        );

        let reg = BuiltinRegistry::new(true, BuiltinProfile::Full);
        reg.record_acl_decision("listener", "tag", "ln", v6);
        let body = encode_builtin(reg.gather());
        assert!(
            body.contains(r#"ip_family="v6""#),
            "full acl decisions include ip_family, body:\n{body}"
        );
        assert!(body.contains(r#"action="tag""#), "body:\n{body}");
    }

    #[test]
    fn minimal_profile_responses_use_coarse_rcode_buckets() {
        let reg = BuiltinRegistry::new(true, BuiltinProfile::Minimal);
        let addr: std::net::SocketAddr = "127.0.0.1:15353".parse().unwrap();
        reg.record_response("ln", "udp", Some(9), &addr, Some("forward"));
        reg.record_response("ln", "udp", Some(0), &addr, Some("cache"));
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
        reg.record_response("ln", "udp", Some(9), &addr, None);
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
    fn lookup_cache_metrics_respect_profile_tiers() {
        for profile in [BuiltinProfile::Minimal, BuiltinProfile::Full] {
            let reg = BuiltinRegistry::new(true, profile);
            let addr: std::net::SocketAddr = "127.0.0.1:15353".parse().unwrap();
            reg.record_lookup_provider_outcome("default", "cache", "answered");
            reg.record_lookup_provider_outcome("default", "forward", "answered");
            reg.record_cache_lookup("global", "default", "hit");
            reg.record_cache_lookup("global", "default", "miss");
            reg.record_cache_fill("global", "default");
            reg.record_cache_singleflight_coalesced("global", "default");
            reg.record_cache_evictions("global", "active_reaper", 3);
            reg.observe_lookup_duration("default", "cache", 0.001);
            reg.observe_cache_lookup_duration("global", "default", 0.0005);
            reg.record_response("ln", "udp", Some(0), &addr, Some("cache"));
            let body = encode_builtin(reg.gather());
            assert!(
                body.contains("conduit_lookup_provider_outcomes_total"),
                "{profile:?} body:\n{body}"
            );
            assert!(
                body.contains(r#"provider="cache""#) && body.contains(r#"profile="default""#),
                "{profile:?} body:\n{body}"
            );
            assert!(
                body.contains("conduit_cache_lookups_total"),
                "{profile:?} body:\n{body}"
            );
            assert!(
                body.contains(r#"answer_source="cache""#),
                "{profile:?} body:\n{body}"
            );
            if profile == BuiltinProfile::Full {
                assert!(body.contains("conduit_cache_fills_total"), "body:\n{body}");
                assert!(
                    body.contains("conduit_cache_singleflight_coalesced_total"),
                    "body:\n{body}"
                );
                assert!(
                    body.contains("conduit_cache_evictions_total")
                        && body.contains(r#"reason="active_reaper""#),
                    "body:\n{body}"
                );
                assert!(
                    body.contains("conduit_lookup_duration_seconds"),
                    "body:\n{body}"
                );
            } else {
                assert!(
                    !body.contains("conduit_cache_fills_total"),
                    "minimal must omit full-only series, body:\n{body}"
                );
                assert!(
                    !body.contains("conduit_cache_evictions_total"),
                    "minimal must omit eviction series, body:\n{body}"
                );
            }
        }
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

    #[test]
    fn health_metrics_exported_only_on_full_profile() {
        let minimal = BuiltinRegistry::new(true, BuiltinProfile::Minimal);
        minimal.set_scrape_snapshot_fn(Arc::new(|| ScrapeGaugeSnapshot {
            health_backends: vec![HealthScrapeBackend {
                pool: "default".into(),
                backend: "127.0.0.1:5300".into(),
                observed: 1.0,
                applied: 2.0,
                probe_automatic: 0.0,
                effective_weight: 0.0,
                latency_ewma_ms: Some(12.5),
                transitions_total: 1,
            }],
            pool_backends_active: vec![("default".into(), 1)],
            ..Default::default()
        }));
        let body_min = encode_builtin(minimal.gather());
        assert!(
            !body_min.contains("conduit_backend_health_observed"),
            "minimal must omit health metrics, body:\n{body_min}"
        );

        let full = BuiltinRegistry::new(true, BuiltinProfile::Full);
        full.set_scrape_snapshot_fn(Arc::new(|| ScrapeGaugeSnapshot {
            health_backends: vec![HealthScrapeBackend {
                pool: "default".into(),
                backend: "127.0.0.1:5300".into(),
                observed: 1.0,
                applied: 2.0,
                probe_automatic: 0.0,
                effective_weight: 0.0,
                latency_ewma_ms: Some(12.5),
                transitions_total: 2,
            }],
            pool_backends_active: vec![("default".into(), 1)],
            pool_backend_counts: vec![("default".into(), 2)],
            ..Default::default()
        }));
        let body_full = encode_builtin(full.gather());
        assert!(
            body_full.contains(
                r#"conduit_backend_health_observed{backend="127.0.0.1:5300",pool="default"} 1"#
            ),
            "body:\n{body_full}"
        );
        assert!(
            body_full.contains(
                r#"conduit_backend_health_applied{backend="127.0.0.1:5300",pool="default"} 2"#
            ),
            "body:\n{body_full}"
        );
        assert!(
            body_full.contains(
                r#"conduit_backend_health_probe_automatic{backend="127.0.0.1:5300",pool="default"} 0"#
            ),
            "body:\n{body_full}"
        );
        assert!(
            body_full.contains(
                r#"conduit_backend_health_effective_weight{backend="127.0.0.1:5300",pool="default"} 0"#
            ),
            "body:\n{body_full}"
        );
        assert!(
            body_full.contains(
                r#"conduit_backend_health_latency_ewma_ms{backend="127.0.0.1:5300",pool="default"} 12.5"#
            ),
            "body:\n{body_full}"
        );
        assert!(
            body_full.contains("conduit_backend_health_transitions_total"),
            "body:\n{body_full}"
        );
        assert!(
            body_full.contains(r#"conduit_pool_backends_active{pool="default"} 1"#),
            "body:\n{body_full}"
        );
        assert!(
            body_full.contains(r#"conduit_pool_backends_configured{pool="default"} 2"#),
            "body:\n{body_full}"
        );
    }

    #[test]
    fn health_fail_open_effective_weight_on_scrape() {
        let full = BuiltinRegistry::new(true, BuiltinProfile::Full);
        full.set_scrape_snapshot_fn(Arc::new(|| ScrapeGaugeSnapshot {
            health_backends: vec![
                HealthScrapeBackend {
                    pool: "default".into(),
                    backend: "127.0.0.1:5300".into(),
                    observed: 2.0,
                    applied: 2.0,
                    probe_automatic: 1.0,
                    effective_weight: 100.0,
                    latency_ewma_ms: None,
                    transitions_total: 0,
                },
                HealthScrapeBackend {
                    pool: "default".into(),
                    backend: "127.0.0.1:5301".into(),
                    observed: 2.0,
                    applied: 2.0,
                    probe_automatic: 1.0,
                    effective_weight: 100.0,
                    latency_ewma_ms: None,
                    transitions_total: 0,
                },
            ],
            pool_backends_active: vec![("default".into(), 0)],
            ..Default::default()
        }));
        let body = encode_builtin(full.gather());
        assert!(
            body.contains(
                r#"conduit_backend_health_effective_weight{backend="127.0.0.1:5300",pool="default"} 100"#
            ),
            "panic fail-open should show configured weight, body:\n{body}"
        );
    }

    fn minimal_plan() -> crate::plan::CompiledMetricsPlan {
        use conduit_proto::config::MetricsConfig;
        let cfg = MetricsConfig {
            enabled: true,
            profile: String::new(),
            prometheus: None,
            otel: None,
            user_metrics: vec![],
            base: "minimal".into(),
            categories: None,
            granularity: None,
            collection: Default::default(),
            event_export: None,
        };
        crate::plan::resolve_metrics_plan(Some(&cfg)).unwrap().plan
    }

    fn standard_plan() -> crate::plan::CompiledMetricsPlan {
        use conduit_proto::config::MetricsConfig;
        let cfg = MetricsConfig {
            enabled: true,
            profile: String::new(),
            prometheus: None,
            otel: None,
            user_metrics: vec![],
            base: "standard".into(),
            categories: None,
            granularity: None,
            collection: Default::default(),
            event_export: None,
        };
        crate::plan::resolve_metrics_plan(Some(&cfg)).unwrap().plan
    }

    /// G1 lab requirement: `base: minimal` includes health series — an
    /// intentional expansion vs the legacy `profile: minimal`, which omitted
    /// health entirely (design §12).
    #[test]
    fn plan_driven_minimal_base_includes_health_series() {
        let reg = BuiltinRegistry::new_from_plan(&minimal_plan());
        assert!(reg.health_enabled(), "base: minimal must enable health");
        reg.set_scrape_snapshot_fn(Arc::new(|| ScrapeGaugeSnapshot {
            health_backends: vec![HealthScrapeBackend {
                pool: "default".into(),
                backend: "127.0.0.1:5300".into(),
                observed: 1.0,
                applied: 1.0,
                probe_automatic: 1.0,
                effective_weight: 100.0,
                latency_ewma_ms: Some(1.5),
                transitions_total: 1,
            }],
            pool_backends_active: vec![("default".into(), 1)],
            ..Default::default()
        }));
        reg.record_probe_result("default", "127.0.0.1:5300", "success");
        let body = encode_builtin(reg.gather());
        assert!(
            body.contains("conduit_backend_health_observed"),
            "base: minimal must expose health gauges, body:\n{body}"
        );
        assert!(
            body.contains("conduit_probe_results_total"),
            "base: minimal must record probe results, body:\n{body}"
        );
        // Still coarse-schema for volume (minimal granularity), unlike standard.
        assert!(
            !body.contains(r#"qtype="#),
            "minimal plan must not add qtype label, body:\n{body}"
        );
    }

    /// Legacy `profile: minimal` (via `new()`) must continue omitting health
    /// — only the new plan-driven path expands minimal to include health.
    #[test]
    fn legacy_minimal_constructor_still_omits_health() {
        let reg = BuiltinRegistry::new(true, BuiltinProfile::Minimal);
        assert!(!reg.health_enabled());
    }

    #[test]
    fn plan_driven_standard_base_matches_full_profile_schema() {
        let reg = BuiltinRegistry::new_from_plan(&standard_plan());
        assert!(reg.health_enabled());
        assert_eq!(reg.profile(), BuiltinProfile::Full);
        let addr: std::net::SocketAddr = "127.0.0.1:15353".parse().unwrap();
        reg.record_query("ln", "udp", Some(1), Some(1), &addr);
        let body = encode_builtin(reg.gather());
        assert!(body.contains(r#"qtype="A""#), "body:\n{body}");
    }

    /// `collection.timing.collect: false` must suppress forward duration
    /// observations even though the timing category remains resolved
    /// (design §"Category collect disabled" scenario).
    #[test]
    fn collect_false_for_timing_suppresses_forward_duration_observations() {
        use conduit_proto::config::{MetricsCollectEmit, MetricsConfig};
        let mut collection = std::collections::HashMap::new();
        collection.insert(
            "timing".to_string(),
            MetricsCollectEmit {
                collect: Some(false),
                emit: Some(false),
            },
        );
        let cfg = MetricsConfig {
            enabled: true,
            profile: String::new(),
            prometheus: None,
            otel: None,
            user_metrics: vec![],
            base: "standard".into(),
            categories: None,
            granularity: None,
            collection,
            event_export: None,
        };
        let plan = crate::plan::resolve_metrics_plan(Some(&cfg)).unwrap().plan;
        assert!(plan
            .categories
            .contains(&crate::plan::MetricCategory::Timing));
        assert!(!plan.collect_for(crate::plan::MetricCategory::Timing));

        let reg = BuiltinRegistry::new_from_plan(&plan);
        reg.record_forward_duration("default", "127.0.0.1:5300", 0.05);
        reg.observe_phase("route", 0.01);
        let body = encode_builtin(reg.gather());
        assert!(
            !body.contains("conduit_forward_duration_seconds_bucket"),
            "collect: false must suppress observations, body:\n{body}"
        );
        assert!(
            !body.contains("conduit_phase_duration_seconds_bucket"),
            "collect: false must suppress observations, body:\n{body}"
        );
    }

    /// `categories.exclude: [failures]` resolves (with a warning, tested in
    /// `plan::tests`) and must suppress failure-category recording.
    #[test]
    fn excluded_failures_category_suppresses_recording() {
        use conduit_proto::config::{MetricsCategories, MetricsConfig};
        let cfg = MetricsConfig {
            enabled: true,
            profile: String::new(),
            prometheus: None,
            otel: None,
            user_metrics: vec![],
            base: "standard".into(),
            categories: Some(MetricsCategories {
                include: vec![],
                exclude: vec!["failures".into()],
            }),
            granularity: None,
            collection: Default::default(),
            event_export: None,
        };
        let plan = crate::plan::resolve_metrics_plan(Some(&cfg)).unwrap().plan;
        assert!(!plan.collect_for(crate::plan::MetricCategory::Failures));

        let reg = BuiltinRegistry::new_from_plan(&plan);
        reg.record_forward_error("default", "127.0.0.1:5300", "timeout");
        reg.record_retry("default");
        let body = encode_builtin(reg.gather());
        assert!(
            !body.contains(r#"conduit_forward_errors_total{backend="127.0.0.1:5300""#),
            "excluded failures category must suppress recording, body:\n{body}"
        );
    }

    fn plan_with_timing_dims(dims: &[&str]) -> crate::plan::CompiledMetricsPlan {
        use conduit_proto::config::{MetricsConfig, MetricsDimensionList, MetricsGranularity};
        let mut overrides = std::collections::HashMap::new();
        overrides.insert(
            "timing".to_string(),
            MetricsDimensionList {
                dimensions: dims.iter().map(|s| (*s).to_string()).collect(),
                rcode: String::new(),
                dimensions_set: true,
            },
        );
        let cfg = MetricsConfig {
            enabled: true,
            profile: String::new(),
            prometheus: None,
            otel: None,
            user_metrics: vec![],
            base: "standard".into(),
            categories: None,
            granularity: Some(MetricsGranularity {
                default: String::new(),
                overrides,
            }),
            collection: Default::default(),
            event_export: None,
        };
        crate::plan::resolve_metrics_plan(Some(&cfg)).unwrap().plan
    }

    #[test]
    fn timing_pool_only_omits_backend_label() {
        let plan = plan_with_timing_dims(&["pool"]);
        assert_eq!(plan.dimensions_for("timing"), &["pool".to_string()]);
        let reg = BuiltinRegistry::new_from_plan(&plan);
        reg.record_forward_duration("default", "127.0.0.1:5300", 0.05);
        reg.record_forward_attempt("default", "127.0.0.1:5300", "success");
        let body = encode_builtin(reg.gather());
        assert!(
            body.contains(r#"conduit_forward_duration_seconds_count{pool="default"}"#)
                || body.contains(r#"conduit_forward_duration_seconds_sum{pool="default"}"#),
            "pool-only duration, body:\n{body}"
        );
        assert!(
            !body.contains(r#"backend="127.0.0.1:5300""#),
            "pool-only must omit backend, body:\n{body}"
        );
        assert!(
            body.contains(r#"conduit_forward_attempts_total{outcome="success",pool="default"}"#),
            "pool-only attempts, body:\n{body}"
        );
    }

    #[test]
    fn timing_pool_backend_includes_both_labels() {
        let plan = plan_with_timing_dims(&["pool", "backend"]);
        let reg = BuiltinRegistry::new_from_plan(&plan);
        reg.record_forward_duration("default", "127.0.0.1:5300", 0.05);
        let body = encode_builtin(reg.gather());
        assert!(
            body.contains(r#"backend="127.0.0.1:5300""#) && body.contains(r#"pool="default""#),
            "pool+backend duration, body:\n{body}"
        );
    }

    #[test]
    fn timing_schema_change_is_distinct_series_identity() {
        // Same metric name, different dimension schemas → distinct MetricStore
        // identities (design §Decisions 8 / series identity).
        let store = crate::MetricStore::new();
        let buckets = vec![0.001, 0.01, 0.1, 1.0];
        let pool_only = store.get_or_create_histogram(
            "conduit_forward_duration_seconds",
            "help",
            &["pool"],
            buckets.clone(),
        );
        let pool_backend = store.get_or_create_histogram(
            "conduit_forward_duration_seconds",
            "help",
            &["pool", "backend"],
            buckets,
        );
        pool_only.with_label_values(&["default"]).observe(0.01);
        pool_backend
            .with_label_values(&["default", "127.0.0.1:5300"])
            .observe(0.02);
        assert_eq!(store.len(), 2);
        assert_eq!(
            pool_only.with_label_values(&["default"]).get_sample_count(),
            1
        );
        assert_eq!(
            pool_backend
                .with_label_values(&["default", "127.0.0.1:5300"])
                .get_sample_count(),
            1
        );
    }

    #[test]
    fn responses_coarse_rcode_override_uses_class_buckets() {
        use conduit_proto::config::{MetricsConfig, MetricsDimensionList, MetricsGranularity};
        let mut overrides = std::collections::HashMap::new();
        overrides.insert(
            "responses".to_string(),
            MetricsDimensionList {
                dimensions: vec![],
                rcode: "coarse".into(),
                dimensions_set: false,
            },
        );
        let cfg = MetricsConfig {
            enabled: true,
            profile: String::new(),
            prometheus: None,
            otel: None,
            user_metrics: vec![],
            base: "standard".into(),
            categories: None,
            granularity: Some(MetricsGranularity {
                default: "fine".into(),
                overrides,
            }),
            collection: Default::default(),
            event_export: None,
        };
        let plan = crate::plan::resolve_metrics_plan(Some(&cfg)).unwrap().plan;
        assert_eq!(
            plan.responses_rcode,
            crate::plan::ResponsesRcodeBucketing::Coarse
        );
        // Fine responses dimensions retained (rcode-only override).
        assert!(plan.has_dimension("responses", "ip_family"));

        let reg = BuiltinRegistry::new_from_plan(&plan);
        let addr: std::net::SocketAddr = "127.0.0.1:15353".parse().unwrap();
        // NXDOMAIN = 3 → coarse bucket still "NXDOMAIN"
        reg.record_response("ln", "udp", Some(3), &addr, Some("forward"));
        let body = encode_builtin(reg.gather());
        assert!(
            body.contains(r#"rcode="NXDOMAIN""#),
            "coarse NXDOMAIN bucket, body:\n{body}"
        );
    }

    #[test]
    fn default_standard_registry_matches_legacy_full_query_labels() {
        let plan_reg = BuiltinRegistry::new_from_plan(&standard_plan());
        let legacy = BuiltinRegistry::new(true, BuiltinProfile::Full);
        let addr: std::net::SocketAddr = "127.0.0.1:15353".parse().unwrap();
        plan_reg.record_query("ln", "udp", Some(1), Some(1), &addr);
        legacy.record_query("ln", "udp", Some(1), Some(1), &addr);
        plan_reg.record_forward_duration("default", "127.0.0.1:5300", 0.01);
        legacy.record_forward_duration("default", "127.0.0.1:5300", 0.01);
        let plan_body = encode_builtin(plan_reg.gather());
        let legacy_body = encode_builtin(legacy.gather());
        let query_line = r#"conduit_queries_total{ip_family="v4",listener="ln",protocol="udp",qclass="IN",qtype="A"}"#;
        assert!(plan_body.contains(query_line), "plan body:\n{plan_body}");
        assert!(
            legacy_body.contains(query_line),
            "legacy body:\n{legacy_body}"
        );
        let timing_line =
            r#"conduit_forward_duration_seconds_count{backend="127.0.0.1:5300",pool="default"}"#;
        assert!(plan_body.contains(timing_line), "plan body:\n{plan_body}");
        assert!(
            legacy_body.contains(timing_line),
            "legacy body:\n{legacy_body}"
        );
    }
}
