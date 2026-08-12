use crate::backend::DEFAULT_BACKEND_WEIGHT;
use crate::defaults::{
    DEFAULT_CONTROL_LISTEN_ADDRESS, DEFAULT_EVENTS_DROP_POLICY, DEFAULT_EVENTS_QUEUE_DEPTH,
    DEFAULT_FORWARD_OUTSTANDING_PER_BACKEND, DEFAULT_FORWARD_TIMEOUT_MS,
    DEFAULT_LISTENER_REUSE_PORT, DEFAULT_LISTENER_THREADS, DEFAULT_ORCHESTRATOR_MAX_ATTEMPTS,
    DEFAULT_ORCHESTRATOR_MAX_TXN_DURATION_MS, DEFAULT_ORCHESTRATOR_TXN_TABLE_CAPACITY,
    DEFAULT_RHAI_MAX_CALL_DEPTH, DEFAULT_RHAI_MAX_OPERATIONS,
};
use crate::error::ConfigError;
use crate::size::parse_si_size;
use conduit_proto::config::{
    AclDeniedSample, AclRule, AclsConfig, Action, Backend, CacheInstance, CacheKeyAugmentConfig,
    CacheKeyConfig, CacheLmdbConfig, CacheMemoryConfig, CacheNegativeConfig, CacheOnHitConfig,
    CacheTruncatedUdpConfig, Config, ControlConfig, ControlTlsConfig, DataSource, DataSourceLimits,
    DataplaneConfig, EventSinkFilters, EventsConfig, ForwardConfig, HealthCheck, Listener,
    ListenersConfig, LoggingConfig, LookupConfig, LookupProfile, LookupProvider, MetricsCategories,
    MetricsCollectEmit, MetricsConfig, MetricsDimensionList, MetricsEventExport,
    MetricsGranularity, OrchestratorConfig, OtelMetricsConfig, Pool, PrometheusMetricsConfig,
    QueryAccessLogging, RhaiConfig, Rule, RulesConfig, Selector, ShutdownConfig, TracingActivation,
    TracingConfig, TracingOutput, UserMetricExportConfig,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct YamlConfig {
    schema_version: u32,
    #[serde(default, skip_serializing_if = "YamlListeners::is_default")]
    listeners: YamlListeners,
    #[serde(default, skip_serializing_if = "YamlForward::is_default")]
    forward: YamlForward,
    #[serde(default, skip_serializing_if = "YamlOrchestrator::is_default")]
    orchestrator: YamlOrchestrator,
    #[serde(default, skip_serializing_if = "YamlEvents::is_default")]
    events: YamlEvents,
    #[serde(default, skip_serializing_if = "YamlRhai::is_default")]
    rhai: YamlRhai,
    pools: Vec<YamlPool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    control: Option<YamlControl>,
    #[serde(default)]
    rules: YamlRules,
    #[serde(default)]
    logging: YamlLogging,
    #[serde(default)]
    data_sources: Vec<YamlDataSource>,
    #[serde(default)]
    metrics: Option<YamlMetrics>,
    #[serde(default)]
    tracing: Option<YamlTracing>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    dataplane: Option<YamlDataplane>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    shutdown: Option<YamlShutdown>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    data_source_limits: Option<YamlDataSourceLimits>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    caches: Vec<YamlCacheInstance>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    lookup: Option<YamlLookup>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    acls: Option<YamlAcls>,
}

/// Sparse overlay patch: omitted top-level keys stay unset (`None` / empty) in [`Config`].
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct YamlOverlayPatch {
    schema_version: u32,
    #[serde(default)]
    listeners: Option<YamlListeners>,
    #[serde(default)]
    forward: Option<YamlForward>,
    #[serde(default)]
    orchestrator: Option<YamlOrchestrator>,
    #[serde(default)]
    events: Option<YamlEvents>,
    #[serde(default)]
    rhai: Option<YamlRhai>,
    #[serde(default)]
    pools: Vec<YamlPool>,
    #[serde(default)]
    control: Option<YamlControl>,
    #[serde(default)]
    rules: Option<YamlRules>,
    #[serde(default)]
    logging: Option<YamlLogging>,
    #[serde(default)]
    data_sources: Vec<YamlDataSource>,
    #[serde(default)]
    metrics: Option<YamlMetrics>,
    #[serde(default)]
    tracing: Option<YamlTracing>,
    #[serde(default)]
    dataplane: Option<YamlDataplane>,
    #[serde(default)]
    shutdown: Option<YamlShutdown>,
    #[serde(default)]
    data_source_limits: Option<YamlDataSourceLimits>,
    #[serde(default)]
    caches: Vec<YamlCacheInstance>,
    #[serde(default)]
    lookup: Option<YamlLookup>,
    #[serde(default)]
    acls: Option<YamlAcls>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct YamlShutdown {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    drain: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    drain_timeout_ms: Option<u32>,
}

impl YamlShutdown {
    fn is_default(&self) -> bool {
        self.drain.is_none() && self.drain_timeout_ms.is_none()
    }
}

#[derive(Debug, Deserialize, Serialize)]
pub(crate) struct YamlLogging {
    #[serde(default = "default_log_level")]
    level: String,
    #[serde(default = "default_log_output")]
    output: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    query_access: Option<YamlQueryAccess>,
}

#[derive(Debug, Deserialize, Serialize, Default, Clone)]
#[serde(deny_unknown_fields)]
pub(crate) struct YamlQueryAccess {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    acl_denied: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    acl_denied_sample: Option<YamlAclDeniedSample>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(deny_unknown_fields)]
pub(crate) struct YamlAclDeniedSample {
    mode: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    rate: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    nth: Option<u32>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(deny_unknown_fields)]
pub(crate) struct YamlAcls {
    #[serde(default = "default_acl_default_action")]
    default_action: String,
    #[serde(default)]
    rules: Vec<YamlAclRule>,
}

fn default_acl_default_action() -> String {
    "allow".into()
}

#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(deny_unknown_fields)]
pub(crate) struct YamlAclRule {
    /// Named `type: cidr` data source.
    #[serde(rename = "match")]
    match_view: String,
    action: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    tag: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
pub(crate) struct YamlDataplane {
    #[serde(default = "default_dataplane_runtime")]
    runtime: String,
    #[serde(default = "default_policy_workers")]
    policy_workers: u32,
    #[serde(default = "default_io_workers")]
    io_workers: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    slot_chunk_size: Option<u32>,
}

fn default_dataplane_runtime() -> String {
    crate::dataplane::DEFAULT_DATAPLANE_RUNTIME.into()
}

fn default_policy_workers() -> u32 {
    crate::dataplane::DEFAULT_POLICY_WORKERS
}

fn default_io_workers() -> u32 {
    crate::dataplane::DEFAULT_IO_WORKERS
}

impl YamlDataplane {
    fn is_sync_default(&self) -> bool {
        self.runtime == crate::dataplane::DEFAULT_DATAPLANE_RUNTIME
            && self.policy_workers == crate::dataplane::DEFAULT_POLICY_WORKERS
            && self.io_workers == crate::dataplane::DEFAULT_IO_WORKERS
            && self.slot_chunk_size.is_none()
    }
}

fn default_log_level() -> String {
    crate::logging::DEFAULT_LOG_LEVEL.into()
}

fn default_log_output() -> String {
    crate::logging::DEFAULT_LOG_OUTPUT.into()
}

impl Default for YamlLogging {
    fn default() -> Self {
        Self {
            level: default_log_level(),
            output: default_log_output(),
            query_access: None,
        }
    }
}

#[derive(Debug, Deserialize, Serialize)]
pub(crate) struct YamlRules {
    #[serde(default = "default_match_mode")]
    match_mode: String,
    #[serde(default)]
    rules: Vec<YamlRule>,
}

fn default_match_mode() -> String {
    "first_match".into()
}

impl Default for YamlRules {
    fn default() -> Self {
        Self {
            match_mode: default_match_mode(),
            rules: Vec::new(),
        }
    }
}

#[derive(Debug, Deserialize, Serialize)]
pub(crate) struct YamlRule {
    name: String,
    hook: String,
    selectors: Vec<YamlSelector>,
    actions: Vec<YamlAction>,
}

#[derive(Debug, Deserialize, Serialize)]
pub(crate) struct YamlSelector {
    #[serde(rename = "type")]
    selector_type: String,
    value: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    key_from: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
pub(crate) struct YamlAction {
    #[serde(rename = "type")]
    action_type: String,
    #[serde(default)]
    value: String,
}

#[derive(Debug, Deserialize, Serialize)]
pub(crate) struct YamlListeners {
    #[serde(default = "default_listener_threads")]
    threads: u32,
    #[serde(default = "default_listener_reuse_port")]
    reuse_port: bool,
    #[serde(default)]
    rcvbuf: u32,
    #[serde(default)]
    sndbuf: u32,
    listeners: Vec<YamlListener>,
}

fn default_listener_threads() -> u32 {
    DEFAULT_LISTENER_THREADS
}

fn default_listener_reuse_port() -> bool {
    DEFAULT_LISTENER_REUSE_PORT
}

impl Default for YamlListeners {
    fn default() -> Self {
        Self {
            threads: DEFAULT_LISTENER_THREADS,
            reuse_port: DEFAULT_LISTENER_REUSE_PORT,
            rcvbuf: 0,
            sndbuf: 0,
            listeners: Vec::new(),
        }
    }
}

impl YamlListeners {
    pub(crate) fn is_default(&self) -> bool {
        self.threads == DEFAULT_LISTENER_THREADS
            && self.reuse_port == DEFAULT_LISTENER_REUSE_PORT
            && self.rcvbuf == 0
            && self.sndbuf == 0
            && self.listeners.is_empty()
    }
}

#[derive(Debug, Deserialize, Serialize)]
pub(crate) struct YamlListener {
    address: String,
    protocol: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    threads: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    reuse_port: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    rcvbuf: Option<u32>,
    /// When set, fully replaces top-level `acls:` for this listener.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    acls: Option<YamlAcls>,
}

fn default_source_selection() -> String {
    crate::forward::DEFAULT_SOURCE_SELECTION.into()
}

#[derive(Debug, Deserialize, Serialize)]
pub(crate) struct YamlForward {
    #[serde(default = "default_forward_outstanding")]
    outstanding_per_backend: u32,
    #[serde(default = "default_forward_timeout_ms")]
    timeout_ms: u32,
    #[serde(default)]
    sources_v4: Vec<String>,
    #[serde(default = "default_source_selection")]
    source_selection: String,
    #[serde(default)]
    sources_v6: Vec<String>,
    #[serde(default)]
    upstream_transport: String,
    #[serde(default)]
    client_tcp_uses_upstream_tcp: bool,
}

fn default_forward_outstanding() -> u32 {
    DEFAULT_FORWARD_OUTSTANDING_PER_BACKEND
}

fn default_forward_timeout_ms() -> u32 {
    DEFAULT_FORWARD_TIMEOUT_MS
}

impl Default for YamlForward {
    fn default() -> Self {
        Self {
            outstanding_per_backend: DEFAULT_FORWARD_OUTSTANDING_PER_BACKEND,
            timeout_ms: DEFAULT_FORWARD_TIMEOUT_MS,
            sources_v4: Vec::new(),
            source_selection: default_source_selection(),
            sources_v6: Vec::new(),
            upstream_transport: String::new(),
            client_tcp_uses_upstream_tcp: false,
        }
    }
}

impl YamlForward {
    pub(crate) fn is_default(&self) -> bool {
        *self == Self::default()
    }
}

impl PartialEq for YamlForward {
    fn eq(&self, other: &Self) -> bool {
        self.outstanding_per_backend == other.outstanding_per_backend
            && self.timeout_ms == other.timeout_ms
            && self.sources_v4 == other.sources_v4
            && self.source_selection == other.source_selection
            && self.sources_v6 == other.sources_v6
            && self.upstream_transport == other.upstream_transport
            && self.client_tcp_uses_upstream_tcp == other.client_tcp_uses_upstream_tcp
    }
}

#[derive(Debug, Deserialize, Serialize)]
pub(crate) struct YamlOrchestrator {
    #[serde(default = "default_orchestrator_max_attempts")]
    max_attempts: u32,
    #[serde(default = "default_orchestrator_max_txn_duration_ms")]
    max_txn_duration_ms: u32,
    #[serde(default = "default_orchestrator_txn_table_capacity")]
    txn_table_capacity: u32,
}

fn default_orchestrator_max_attempts() -> u32 {
    DEFAULT_ORCHESTRATOR_MAX_ATTEMPTS
}

fn default_orchestrator_max_txn_duration_ms() -> u32 {
    DEFAULT_ORCHESTRATOR_MAX_TXN_DURATION_MS
}

fn default_orchestrator_txn_table_capacity() -> u32 {
    DEFAULT_ORCHESTRATOR_TXN_TABLE_CAPACITY
}

impl Default for YamlOrchestrator {
    fn default() -> Self {
        Self {
            max_attempts: DEFAULT_ORCHESTRATOR_MAX_ATTEMPTS,
            max_txn_duration_ms: DEFAULT_ORCHESTRATOR_MAX_TXN_DURATION_MS,
            txn_table_capacity: DEFAULT_ORCHESTRATOR_TXN_TABLE_CAPACITY,
        }
    }
}

impl YamlOrchestrator {
    pub(crate) fn is_default(&self) -> bool {
        *self == Self::default()
    }
}

impl PartialEq for YamlOrchestrator {
    fn eq(&self, other: &Self) -> bool {
        self.max_attempts == other.max_attempts
            && self.max_txn_duration_ms == other.max_txn_duration_ms
            && self.txn_table_capacity == other.txn_table_capacity
    }
}

#[derive(Debug, Deserialize, Serialize)]
pub(crate) struct YamlEvents {
    #[serde(default = "default_events_queue_depth")]
    queue_depth: u32,
    #[serde(default = "default_events_drop_policy")]
    drop_policy: String,
    #[serde(default)]
    sinks: Vec<YamlEventSink>,
}

fn default_events_queue_depth() -> u32 {
    DEFAULT_EVENTS_QUEUE_DEPTH
}

fn default_events_drop_policy() -> String {
    DEFAULT_EVENTS_DROP_POLICY.into()
}

impl Default for YamlEvents {
    fn default() -> Self {
        Self {
            queue_depth: DEFAULT_EVENTS_QUEUE_DEPTH,
            drop_policy: DEFAULT_EVENTS_DROP_POLICY.into(),
            sinks: Vec::new(),
        }
    }
}

impl YamlEvents {
    pub(crate) fn is_default(&self) -> bool {
        self.queue_depth == DEFAULT_EVENTS_QUEUE_DEPTH
            && self.drop_policy == DEFAULT_EVENTS_DROP_POLICY
            && self.sinks.is_empty()
    }
}

#[derive(Debug, Deserialize, Serialize, Default)]
pub(crate) struct YamlEventSinkFilters {
    #[serde(default)]
    tag_required: Option<String>,
    #[serde(default)]
    selectors: Vec<YamlSelector>,
    #[serde(default)]
    sample_percent: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    sample_key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    sample_key_from: Option<String>,
    #[serde(default)]
    pool: Option<String>,
    #[serde(default)]
    backend: Option<String>,
}

fn yaml_event_filters_nonempty(f: &YamlEventSinkFilters) -> bool {
    f.tag_required.is_some()
        || !f.selectors.is_empty()
        || f.sample_percent.is_some()
        || f.sample_key.as_ref().is_some_and(|k| !k.is_empty())
        || f.sample_key_from.as_ref().is_some_and(|k| !k.is_empty())
        || f.pool.as_ref().is_some_and(|p| !p.is_empty())
        || f.backend.as_ref().is_some_and(|b| !b.is_empty())
}

#[derive(Debug, Deserialize, Serialize, Default)]
pub(crate) struct YamlConnectRetry {
    #[serde(default)]
    initial_ms: u32,
    #[serde(default)]
    max_ms: u32,
    #[serde(default)]
    multiplier: f64,
    #[serde(default)]
    max_elapsed_ms: u32,
    #[serde(default = "default_connect_retry_jitter")]
    jitter: bool,
}

fn default_connect_retry_jitter() -> bool {
    true
}

#[derive(Debug, Deserialize, Serialize)]
pub(crate) struct YamlEventSink {
    #[serde(rename = "type")]
    sink_type: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    export_id: String,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    destinations: Vec<String>,
    #[serde(default)]
    emit: Vec<String>,
    #[serde(default)]
    filters: YamlEventSinkFilters,
    #[serde(default)]
    extra_fields: Vec<String>,
    #[serde(default)]
    extra_tags: Vec<String>,
    #[serde(default)]
    connect_retry: Option<YamlConnectRetry>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct YamlDataSource {
    name: String,
    #[serde(rename = "type")]
    source_type: String,
    path: String,
    #[serde(default)]
    key_column: String,
    #[serde(default)]
    value_column: String,
    // Optional per-entry load-safety overrides; unset = inherit data_source_limits.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    max_file_bytes: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    max_entries: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    max_key_bytes: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    max_value_bytes: Option<u32>,
}

/// Generic load-safety limits for `data_sources` (table/key-value abstraction;
/// applies to any source type). `0` on any field means "use the built-in
/// default" at load time.
#[derive(Debug, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct YamlDataSourceLimits {
    #[serde(default, skip_serializing_if = "is_zero_u64")]
    max_file_bytes: u64,
    #[serde(default, skip_serializing_if = "is_zero_u64")]
    max_entries: u64,
    #[serde(default, skip_serializing_if = "is_zero_u32")]
    max_key_bytes: u32,
    #[serde(default, skip_serializing_if = "is_zero_u32")]
    max_value_bytes: u32,
    #[serde(default, skip_serializing_if = "is_zero_u32")]
    max_tables: u32,
    #[serde(default, skip_serializing_if = "is_zero_u64")]
    max_total_bytes: u64,
}

impl YamlDataSourceLimits {
    fn is_default(&self) -> bool {
        self.max_file_bytes == 0
            && self.max_entries == 0
            && self.max_key_bytes == 0
            && self.max_value_bytes == 0
            && self.max_tables == 0
            && self.max_total_bytes == 0
    }
}

fn is_zero_u64(v: &u64) -> bool {
    *v == 0
}

fn is_zero_u32(v: &u32) -> bool {
    *v == 0
}

#[derive(Debug, Deserialize, Serialize)]
pub(crate) struct YamlRhai {
    #[serde(default = "default_rhai_max_operations")]
    max_operations: u64,
    #[serde(default = "default_rhai_max_call_depth")]
    max_call_depth: u32,
    #[serde(default)]
    hook_timeout_ms: u32,
}

fn default_rhai_max_operations() -> u64 {
    DEFAULT_RHAI_MAX_OPERATIONS
}

fn default_rhai_max_call_depth() -> u32 {
    DEFAULT_RHAI_MAX_CALL_DEPTH
}

impl Default for YamlRhai {
    fn default() -> Self {
        Self {
            max_operations: DEFAULT_RHAI_MAX_OPERATIONS,
            max_call_depth: DEFAULT_RHAI_MAX_CALL_DEPTH,
            hook_timeout_ms: 0,
        }
    }
}

impl YamlRhai {
    pub(crate) fn is_default(&self) -> bool {
        *self == Self::default()
    }
}

impl PartialEq for YamlRhai {
    fn eq(&self, other: &Self) -> bool {
        self.max_operations == other.max_operations
            && self.max_call_depth == other.max_call_depth
            && self.hook_timeout_ms == other.hook_timeout_ms
    }
}

#[derive(Debug, Deserialize, Serialize)]
pub(crate) struct YamlPool {
    name: String,
    #[serde(default)]
    backends: Vec<YamlBackend>,
    #[serde(default)]
    sources_v4: Vec<String>,
    #[serde(default)]
    sources_v6: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    max_inflight: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    health: Option<YamlHealthCheck>,
    /// Overlay-only remove marker; never serialized from effective export.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    remove: Option<bool>,
}

#[derive(Debug, Deserialize, Serialize, Default)]
#[serde(deny_unknown_fields)]
pub(crate) struct YamlHealthCheck {
    #[serde(default)]
    enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    interval_ms: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    timeout_ms: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    rise: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    fall: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    probe_qname: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    probe_qtype: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    acceptable_rcodes: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    initial_state: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    latency_weighting: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    min_eligible: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    passive_fast_trip: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    passive_fall: Option<u32>,
}

#[derive(Debug, Deserialize, Serialize)]
pub(crate) struct YamlBackend {
    #[serde(default)]
    address: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    name: Option<String>,
    /// Omitted in YAML means default weight (100).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    weight: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    probe_qname: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    probe_qtype: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    probe_source: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    transport: Option<String>,
    /// Overlay-only remove marker; never serialized from effective export.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    remove: Option<bool>,
}

#[derive(Debug, Deserialize, Serialize)]
pub(crate) struct YamlUserMetricExport {
    name: String,
    /// Deprecated alias for collect/emit; empty means unset.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    export: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    collect: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    emit: Option<bool>,
    /// Prometheus HELP / OTel description; empty means default at export.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    help: String,
}

#[derive(Debug, Deserialize, Serialize)]
pub(crate) struct YamlMetrics {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    enabled: Option<bool>,
    /// Deprecated alias for `base`; empty means unset. Retained through the
    /// 1.x line (metrics-configurability design §Migration Plan).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    profile: String,
    /// "none" | "minimal" | "standard"; empty means unset (defaults to
    /// "standard" when enabled).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    base: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    categories: Option<YamlMetricsCategories>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    granularity: Option<YamlGranularity>,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    collection: std::collections::HashMap<String, YamlCollectEmit>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    event_export: Option<YamlCollectEmit>,
    #[serde(default)]
    prometheus: Option<YamlPrometheusMetrics>,
    #[serde(default)]
    otel: Option<YamlOtelMetrics>,
    #[serde(default)]
    user_metrics: Vec<YamlUserMetricExport>,
}

#[derive(Debug, Deserialize, Serialize, Default)]
pub(crate) struct YamlMetricsCategories {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    include: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    exclude: Option<Vec<String>>,
}

/// `default` is the granularity preset; any other key names a per-family
/// override. Most families use a dimension list (`timing: [pool]`); the
/// `responses` family may also be a map with `dimensions` and/or `rcode`
/// (`coarse` | `iana`) for orthogonal rcode bucketing.
#[derive(Debug, Deserialize, Serialize, Default)]
pub(crate) struct YamlGranularity {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    default: String,
    #[serde(flatten)]
    overrides: std::collections::HashMap<String, YamlFamilyGranularity>,
}

/// Per-family granularity override: bare dimension list, or (for responses)
/// a map with optional `dimensions` and `rcode`.
#[derive(Debug, Deserialize, Serialize, Clone, PartialEq, Eq)]
#[serde(untagged)]
pub(crate) enum YamlFamilyGranularity {
    Dimensions(Vec<String>),
    Detailed {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        dimensions: Option<Vec<String>>,
        #[serde(default, skip_serializing_if = "String::is_empty")]
        rcode: String,
    },
}

impl YamlFamilyGranularity {
    fn into_proto(self) -> MetricsDimensionList {
        match self {
            YamlFamilyGranularity::Dimensions(dimensions) => MetricsDimensionList {
                dimensions,
                rcode: String::new(),
                dimensions_set: true,
            },
            YamlFamilyGranularity::Detailed { dimensions, rcode } => MetricsDimensionList {
                dimensions_set: dimensions.is_some(),
                dimensions: dimensions.unwrap_or_default(),
                rcode,
            },
        }
    }

    fn from_proto(list: &MetricsDimensionList) -> Self {
        if !list.rcode.is_empty() || !list.dimensions_set {
            YamlFamilyGranularity::Detailed {
                dimensions: if list.dimensions_set {
                    Some(list.dimensions.clone())
                } else {
                    None
                },
                rcode: list.rcode.clone(),
            }
        } else {
            YamlFamilyGranularity::Dimensions(list.dimensions.clone())
        }
    }
}

#[derive(Debug, Deserialize, Serialize, Default, Clone, Copy)]
pub(crate) struct YamlCollectEmit {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    collect: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    emit: Option<bool>,
}

#[derive(Debug, Deserialize, Serialize)]
pub(crate) struct YamlPrometheusMetrics {
    /// Empty means unset on overlay merge; file validate may still require it
    /// when a prometheus block is present.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    listen_address: String,
    /// Empty means unset on overlay merge; compile defaults empty to `/metrics`.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    path: String,
}

#[derive(Debug, Deserialize, Serialize)]
pub(crate) struct YamlOtelMetrics {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    endpoint: String,
    /// Omitted in sparse overlays means keep baseline (`None` → proto `0`).
    /// File compile maps `0`/`None` to 15000.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    push_interval_ms: Option<u32>,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    resource_attributes: std::collections::HashMap<String, String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    allow_invalid_certs: Option<bool>,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    headers: std::collections::HashMap<String, String>,
}

#[derive(Debug, Deserialize, Serialize)]
pub(crate) struct YamlTracing {
    #[serde(default)]
    enabled: bool,
    #[serde(default)]
    activation: YamlTracingActivation,
    #[serde(default)]
    output: YamlTracingOutput,
}

#[derive(Debug, Deserialize, Serialize, Default)]
pub(crate) struct YamlTracingActivation {
    #[serde(default)]
    tag: Option<String>,
    #[serde(default)]
    selectors: Vec<YamlSelector>,
    #[serde(default)]
    sample_percent: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    sample_key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    sample_key_from: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, Default)]
pub(crate) struct YamlTracingOutput {
    #[serde(default)]
    log_json: bool,
}

#[derive(Debug, Deserialize, Serialize)]
pub(crate) struct YamlControlTls {
    cert_path: String,
    key_path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    client_ca_path: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
pub(crate) struct YamlControl {
    #[serde(default = "default_control_listen_address")]
    listen_address: String,
    #[serde(default, skip_serializing_if = "is_false")]
    reflection_enabled: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    api_keys: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    tls: Option<YamlControlTls>,
}

fn default_control_listen_address() -> String {
    DEFAULT_CONTROL_LISTEN_ADDRESS.into()
}

impl Default for YamlControl {
    fn default() -> Self {
        Self {
            listen_address: default_control_listen_address(),
            reflection_enabled: false,
            api_keys: Vec::new(),
            tls: None,
        }
    }
}

fn is_false(v: &bool) -> bool {
    !*v
}

#[derive(Debug, Deserialize, Serialize, Default)]
pub(crate) struct YamlLookup {
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    profiles: HashMap<String, YamlLookupProfile>,
}

impl YamlLookup {
    fn is_empty(&self) -> bool {
        self.profiles.is_empty()
    }
}

#[derive(Debug, Deserialize, Serialize)]
pub(crate) struct YamlLookupProfile {
    providers: Vec<YamlLookupProvider>,
}

#[derive(Debug, Deserialize, Serialize)]
pub(crate) struct YamlLookupProvider {
    #[serde(rename = "type")]
    provider_type: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    cache: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
pub(crate) struct YamlCacheInstance {
    name: String,
    #[serde(rename = "type")]
    cache_type: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    negative_cache: Option<YamlCacheNegativeConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    on_hit: Option<YamlCacheOnHitConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    truncated_udp: Option<YamlCacheTruncatedUdpConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    rotate_rrset_on_serve: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    memory: Option<YamlCacheMemoryConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    lmdb: Option<YamlCacheLmdbConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    key: Option<YamlCacheKeyConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    max_entries: Option<u64>,
}

#[derive(Debug, Deserialize, Serialize, Default)]
pub(crate) struct YamlCacheNegativeConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    enabled: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    nxdomain_covers_descendants: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    servfail_ttl_secs: Option<u32>,
}

#[derive(Debug, Deserialize, Serialize, Default)]
pub(crate) struct YamlCacheTruncatedUdpConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    enabled: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    ttl_secs: Option<u32>,
}

#[derive(Debug, Deserialize, Serialize)]
pub(crate) struct YamlCacheOnHitConfig {
    response_rules: String,
}

#[derive(Debug, Deserialize, Serialize)]
pub(crate) struct YamlCacheMemoryConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    shard_count: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    eviction: Option<String>,
}

/// YAML `lmdb.map_size`: bare integer bytes or SI string (`4GB`, `4.5GB`).
#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(untagged)]
pub(crate) enum YamlMapSize {
    Bytes(u64),
    Si(String),
}

#[derive(Debug, Deserialize, Serialize)]
pub(crate) struct YamlCacheLmdbConfig {
    path: String,
    map_size: YamlMapSize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    when_full: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    sample_size: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    shard_count: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    sync: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    sync_interval: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, Default)]
pub(crate) struct YamlCacheKeyConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    augment: Option<YamlCacheKeyAugmentConfig>,
}

#[derive(Debug, Deserialize, Serialize, Default)]
pub(crate) struct YamlCacheKeyAugmentConfig {}

pub fn load_yaml(input: &str) -> Result<Config, ConfigError> {
    let y: YamlConfig = serde_yaml::from_str(input)?;
    config_from_yaml(y)
}

/// Parse a sparse `metrics` config from YAML (same field shape as the config-file
/// `metrics:` block).
///
/// Accepts either a bare metrics object (`enabled: true`, `base: …`, …) or a
/// document with a top-level `metrics:` key. Used by `conduitctl metrics patch
/// --file`.
pub fn load_metrics_yaml(input: &str) -> Result<MetricsConfig, ConfigError> {
    let value: serde_yaml::Value = serde_yaml::from_str(input)?;
    let metrics_value = match &value {
        serde_yaml::Value::Mapping(map) => {
            let key = serde_yaml::Value::String("metrics".into());
            if map.contains_key(&key) {
                map.get(&key).cloned().unwrap_or(serde_yaml::Value::Null)
            } else {
                value
            }
        }
        _ => value,
    };
    let y: YamlMetrics = serde_yaml::from_value(metrics_value)?;
    Ok(y.into())
}

/// Serialize a metrics config to operator-facing YAML (bare object fields — the
/// same shape accepted by [`load_metrics_yaml`]).
pub fn export_metrics_yaml(metrics: &MetricsConfig) -> Result<String, ConfigError> {
    let y = YamlMetrics::from(metrics);
    serde_yaml::to_string(&y).map_err(ConfigError::from)
}

/// Load a sparse YAML overlay patch for `conduitctl apply`.
///
/// Omitted top-level sections remain unset in the returned [`Config`], unlike [`load_yaml`]
/// which materializes file-layer defaults for a startup config document.
pub fn load_overlay_patch(input: &str) -> Result<Config, ConfigError> {
    let y: YamlOverlayPatch = serde_yaml::from_str(input)?;
    let cfg = config_from_overlay(y)?;
    let validation = crate::overlay::validate_overlay_patch(&cfg);
    if !validation.ok {
        return Err(ConfigError::Invalid(validation.errors.join("; ")));
    }
    Ok(cfg)
}

fn config_from_overlay(y: YamlOverlayPatch) -> Result<Config, ConfigError> {
    Ok(Config {
        schema_version: y.schema_version,
        listeners: y.listeners.map(Into::into),
        forward: y.forward.map(Into::into),
        orchestrator: y.orchestrator.map(Into::into),
        events: y.events.map(Into::into),
        rhai: y.rhai.map(Into::into),
        pools: y.pools.into_iter().map(Into::into).collect(),
        control: y.control.map(Into::into),
        rules: y.rules.map(Into::into),
        logging: y.logging.map(Into::into),
        data_sources: y.data_sources.into_iter().map(Into::into).collect(),
        metrics: y.metrics.map(Into::into),
        tracing: y.tracing.map(Into::into),
        dataplane: y.dataplane.map(Into::into),
        shutdown: y.shutdown.map(Into::into),
        data_source_limits: y.data_source_limits.map(Into::into),
        caches: y
            .caches
            .into_iter()
            .map(cache_instance_from_yaml)
            .collect::<Result<_, _>>()?,
        lookup: y.lookup.map(Into::into),
        acls: y.acls.map(Into::into),
    })
}

fn config_from_yaml(y: YamlConfig) -> Result<Config, ConfigError> {
    Ok(Config {
        schema_version: y.schema_version,
        listeners: Some(y.listeners.into()),
        forward: Some(y.forward.into()),
        orchestrator: Some(y.orchestrator.into()),
        events: Some(y.events.into()),
        rhai: Some(y.rhai.into()),
        pools: y.pools.into_iter().map(Into::into).collect(),
        control: y.control.map(Into::into),
        rules: Some(y.rules.into()),
        logging: Some(y.logging.into()),
        data_sources: y.data_sources.into_iter().map(Into::into).collect(),
        metrics: y.metrics.map(Into::into),
        tracing: y.tracing.map(Into::into),
        dataplane: y.dataplane.map(Into::into),
        shutdown: y.shutdown.map(Into::into),
        data_source_limits: y.data_source_limits.map(Into::into),
        caches: y
            .caches
            .into_iter()
            .map(cache_instance_from_yaml)
            .collect::<Result<_, _>>()?,
        lookup: y.lookup.map(Into::into),
        acls: y.acls.map(Into::into),
    })
}

fn cache_instance_from_yaml(y: YamlCacheInstance) -> Result<CacheInstance, ConfigError> {
    Ok(CacheInstance {
        name: y.name,
        r#type: y.cache_type,
        negative_cache: y.negative_cache.map(Into::into),
        on_hit: y.on_hit.map(Into::into),
        truncated_udp: y.truncated_udp.map(Into::into),
        rotate_rrset_on_serve: y.rotate_rrset_on_serve,
        memory: y.memory.map(Into::into),
        lmdb: y.lmdb.map(cache_lmdb_from_yaml).transpose()?,
        key: y.key.map(Into::into),
        max_entries: y.max_entries,
    })
}

fn cache_lmdb_from_yaml(y: YamlCacheLmdbConfig) -> Result<CacheLmdbConfig, ConfigError> {
    let map_size_bytes = match y.map_size {
        YamlMapSize::Bytes(n) => n,
        YamlMapSize::Si(ref s) => parse_si_size(s).map_err(ConfigError::Invalid)?,
    };
    Ok(CacheLmdbConfig {
        path: y.path,
        map_size_bytes,
        when_full: y.when_full,
        sample_size: y.sample_size,
        shard_count: y.shard_count,
        sync: y.sync,
        sync_interval: y.sync_interval,
    })
}

impl From<YamlOverlayPatch> for Config {
    fn from(y: YamlOverlayPatch) -> Self {
        // Fallible cache conversion lives in [`config_from_overlay`]; this path is
        // retained for callers that only need non-cache overlay fields.
        config_from_overlay(y).expect("overlay without invalid lmdb.map_size")
    }
}

impl From<YamlConfig> for Config {
    fn from(y: YamlConfig) -> Self {
        config_from_yaml(y).expect("config without invalid lmdb.map_size")
    }
}

impl From<YamlShutdown> for ShutdownConfig {
    fn from(y: YamlShutdown) -> Self {
        ShutdownConfig {
            drain: y.drain,
            drain_timeout_ms: y.drain_timeout_ms,
        }
    }
}

impl From<YamlDataplane> for DataplaneConfig {
    fn from(y: YamlDataplane) -> Self {
        DataplaneConfig {
            runtime: y.runtime,
            policy_workers: y.policy_workers,
            io_workers: y.io_workers,
            slot_chunk_size: y.slot_chunk_size,
        }
    }
}

impl From<YamlMetrics> for MetricsConfig {
    fn from(y: YamlMetrics) -> Self {
        MetricsConfig {
            enabled: y.enabled,
            profile: y.profile,
            prometheus: y.prometheus.map(Into::into),
            otel: y.otel.map(Into::into),
            user_metrics: y
                .user_metrics
                .into_iter()
                .map(|u| UserMetricExportConfig {
                    name: u.name,
                    export: u.export,
                    collect: u.collect,
                    emit: u.emit,
                    help: u.help,
                })
                .collect(),
            base: y.base,
            categories: y.categories.map(|c| MetricsCategories {
                include_set: c.include.is_some(),
                exclude_set: c.exclude.is_some(),
                include: c.include.unwrap_or_default(),
                exclude: c.exclude.unwrap_or_default(),
            }),
            granularity: y.granularity.map(|g| MetricsGranularity {
                default: g.default,
                overrides: g
                    .overrides
                    .into_iter()
                    .map(|(family, override_)| (family, override_.into_proto()))
                    .collect(),
            }),
            collection: y
                .collection
                .into_iter()
                .map(|(name, ce)| {
                    (
                        name,
                        MetricsCollectEmit {
                            collect: ce.collect,
                            emit: ce.emit,
                        },
                    )
                })
                .collect(),
            event_export: y.event_export.map(|ce| MetricsEventExport {
                collect: ce.collect,
                emit: ce.emit,
            }),
        }
    }
}

impl From<YamlPrometheusMetrics> for PrometheusMetricsConfig {
    fn from(y: YamlPrometheusMetrics) -> Self {
        PrometheusMetricsConfig {
            listen_address: y.listen_address,
            path: y.path,
        }
    }
}

impl From<YamlOtelMetrics> for OtelMetricsConfig {
    fn from(y: YamlOtelMetrics) -> Self {
        OtelMetricsConfig {
            endpoint: y.endpoint,
            // 0 means unset for overlay merge; compile maps 0 → 15000.
            push_interval_ms: y.push_interval_ms.unwrap_or(0),
            resource_attributes: y.resource_attributes,
            allow_invalid_certs: y.allow_invalid_certs,
            headers: y.headers,
        }
    }
}

impl From<YamlTracing> for TracingConfig {
    fn from(y: YamlTracing) -> Self {
        TracingConfig {
            enabled: y.enabled,
            activation: Some(y.activation.into()),
            output: Some(y.output.into()),
        }
    }
}

impl From<YamlTracingActivation> for TracingActivation {
    fn from(y: YamlTracingActivation) -> Self {
        TracingActivation {
            tag: y.tag,
            selectors: y.selectors.into_iter().map(Into::into).collect(),
            sample_percent: y.sample_percent,
            sample_key: y.sample_key,
            sample_key_from: y.sample_key_from,
        }
    }
}

impl From<YamlTracingOutput> for TracingOutput {
    fn from(y: YamlTracingOutput) -> Self {
        TracingOutput {
            log_json: y.log_json,
        }
    }
}

impl From<YamlLogging> for LoggingConfig {
    fn from(y: YamlLogging) -> Self {
        LoggingConfig {
            level: y.level,
            output: y.output,
            query_access: y.query_access.map(Into::into),
        }
    }
}

impl From<YamlQueryAccess> for QueryAccessLogging {
    fn from(y: YamlQueryAccess) -> Self {
        QueryAccessLogging {
            acl_denied: y.acl_denied,
            acl_denied_sample: y.acl_denied_sample.map(Into::into),
        }
    }
}

impl From<YamlAclDeniedSample> for AclDeniedSample {
    fn from(y: YamlAclDeniedSample) -> Self {
        AclDeniedSample {
            mode: y.mode,
            rate: y.rate,
            nth: y.nth,
        }
    }
}

impl From<YamlAcls> for AclsConfig {
    fn from(y: YamlAcls) -> Self {
        AclsConfig {
            default_action: y.default_action,
            rules: y.rules.into_iter().map(Into::into).collect(),
        }
    }
}

impl From<YamlAclRule> for AclRule {
    fn from(y: YamlAclRule) -> Self {
        AclRule {
            r#match: y.match_view,
            action: y.action,
            tag: y.tag,
        }
    }
}

impl From<YamlRules> for RulesConfig {
    fn from(y: YamlRules) -> Self {
        RulesConfig {
            match_mode: y.match_mode,
            rules: y.rules.into_iter().map(Into::into).collect(),
        }
    }
}

impl From<YamlRule> for Rule {
    fn from(y: YamlRule) -> Self {
        Rule {
            name: y.name,
            hook: y.hook,
            selectors: y.selectors.into_iter().map(Into::into).collect(),
            actions: y.actions.into_iter().map(Into::into).collect(),
        }
    }
}

impl From<YamlSelector> for Selector {
    fn from(y: YamlSelector) -> Self {
        Selector {
            r#type: y.selector_type,
            value: y.value,
            key: y.key,
            key_from: y.key_from,
        }
    }
}

impl From<YamlAction> for Action {
    fn from(y: YamlAction) -> Self {
        Action {
            r#type: y.action_type,
            value: y.value,
        }
    }
}

impl From<YamlListeners> for ListenersConfig {
    fn from(y: YamlListeners) -> Self {
        ListenersConfig {
            threads: y.threads,
            reuse_port: y.reuse_port,
            rcvbuf: y.rcvbuf,
            sndbuf: y.sndbuf,
            listeners: y.listeners.into_iter().map(Into::into).collect(),
        }
    }
}

impl From<YamlListener> for Listener {
    fn from(y: YamlListener) -> Self {
        Listener {
            address: y.address,
            protocol: y.protocol,
            threads: y.threads,
            reuse_port: y.reuse_port,
            name: y.name,
            rcvbuf: y.rcvbuf,
            acls: y.acls.map(Into::into),
        }
    }
}

impl From<YamlForward> for ForwardConfig {
    fn from(y: YamlForward) -> Self {
        ForwardConfig {
            outstanding_per_backend: y.outstanding_per_backend,
            timeout_ms: y.timeout_ms,
            sources_v4: y.sources_v4,
            source_selection: y.source_selection,
            sources_v6: y.sources_v6,
            upstream_transport: y.upstream_transport,
            client_tcp_uses_upstream_tcp: y.client_tcp_uses_upstream_tcp,
        }
    }
}

impl From<YamlOrchestrator> for OrchestratorConfig {
    fn from(y: YamlOrchestrator) -> Self {
        OrchestratorConfig {
            max_attempts: y.max_attempts,
            max_txn_duration_ms: y.max_txn_duration_ms,
            txn_table_capacity: y.txn_table_capacity,
        }
    }
}

impl From<YamlEvents> for EventsConfig {
    fn from(y: YamlEvents) -> Self {
        EventsConfig {
            queue_depth: y.queue_depth,
            drop_policy: y.drop_policy,
            sinks: y.sinks.into_iter().map(Into::into).collect(),
        }
    }
}

impl From<YamlEventSink> for conduit_proto::config::EventSink {
    fn from(y: YamlEventSink) -> Self {
        let filters = if yaml_event_filters_nonempty(&y.filters) {
            Some(EventSinkFilters {
                tag_required: y.filters.tag_required.clone(),
                selectors: y
                    .filters
                    .selectors
                    .iter()
                    .map(|s| Selector {
                        r#type: s.selector_type.clone(),
                        value: s.value.clone(),
                        key: s.key.clone(),
                        key_from: s.key_from.clone(),
                    })
                    .collect(),
                sample_percent: y.filters.sample_percent,
                sample_key: y.filters.sample_key.clone(),
                sample_key_from: y.filters.sample_key_from.clone(),
                pool: y.filters.pool.clone(),
                backend: y.filters.backend.clone(),
            })
        } else {
            None
        };
        let connect_retry = y
            .connect_retry
            .map(|r| conduit_proto::config::ConnectRetry {
                initial_ms: r.initial_ms,
                max_ms: r.max_ms,
                multiplier: r.multiplier,
                max_elapsed_ms: r.max_elapsed_ms,
                jitter: r.jitter,
            });
        conduit_proto::config::EventSink {
            r#type: y.sink_type,
            export_id: y.export_id,
            name: y.name,
            destinations: y.destinations,
            emit: y.emit,
            filters,
            extra_fields: y.extra_fields,
            extra_tags: y.extra_tags,
            connect_retry,
        }
    }
}

impl From<YamlRhai> for RhaiConfig {
    fn from(y: YamlRhai) -> Self {
        RhaiConfig {
            max_operations: y.max_operations,
            max_call_depth: y.max_call_depth,
            hook_timeout_ms: y.hook_timeout_ms,
        }
    }
}

impl From<YamlDataSource> for DataSource {
    fn from(y: YamlDataSource) -> Self {
        DataSource {
            name: y.name,
            r#type: y.source_type,
            path: y.path,
            key_column: y.key_column,
            value_column: y.value_column,
            max_file_bytes: y.max_file_bytes,
            max_entries: y.max_entries,
            max_key_bytes: y.max_key_bytes,
            max_value_bytes: y.max_value_bytes,
        }
    }
}

impl From<YamlDataSourceLimits> for DataSourceLimits {
    fn from(y: YamlDataSourceLimits) -> Self {
        DataSourceLimits {
            max_file_bytes: y.max_file_bytes,
            max_entries: y.max_entries,
            max_key_bytes: y.max_key_bytes,
            max_value_bytes: y.max_value_bytes,
            max_tables: y.max_tables,
            max_total_bytes: y.max_total_bytes,
        }
    }
}

impl From<&DataSourceLimits> for YamlDataSourceLimits {
    fn from(d: &DataSourceLimits) -> Self {
        YamlDataSourceLimits {
            max_file_bytes: d.max_file_bytes,
            max_entries: d.max_entries,
            max_key_bytes: d.max_key_bytes,
            max_value_bytes: d.max_value_bytes,
            max_tables: d.max_tables,
            max_total_bytes: d.max_total_bytes,
        }
    }
}

impl From<YamlPool> for Pool {
    fn from(y: YamlPool) -> Self {
        Pool {
            name: y.name,
            backends: y.backends.into_iter().map(Into::into).collect(),
            sources_v4: y.sources_v4,
            sources_v6: y.sources_v6,
            max_inflight: y.max_inflight,
            health: y.health.map(Into::into),
            remove: y.remove.filter(|r| *r),
        }
    }
}

impl From<YamlHealthCheck> for HealthCheck {
    fn from(y: YamlHealthCheck) -> Self {
        HealthCheck {
            enabled: y.enabled,
            interval_ms: y.interval_ms,
            timeout_ms: y.timeout_ms,
            rise: y.rise,
            fall: y.fall,
            probe_qname: y.probe_qname,
            probe_qtype: y.probe_qtype,
            acceptable_rcodes: y.acceptable_rcodes,
            initial_state: y.initial_state,
            latency_weighting: y.latency_weighting,
            min_eligible: y.min_eligible,
            passive_fast_trip: y.passive_fast_trip,
            passive_fall: y.passive_fall,
        }
    }
}

impl From<&HealthCheck> for YamlHealthCheck {
    fn from(h: &HealthCheck) -> Self {
        YamlHealthCheck {
            enabled: h.enabled,
            interval_ms: h.interval_ms,
            timeout_ms: h.timeout_ms,
            rise: h.rise,
            fall: h.fall,
            probe_qname: h.probe_qname.clone(),
            probe_qtype: h.probe_qtype.clone(),
            acceptable_rcodes: h.acceptable_rcodes.clone(),
            initial_state: h.initial_state.clone(),
            latency_weighting: h.latency_weighting,
            min_eligible: h.min_eligible,
            passive_fast_trip: h.passive_fast_trip,
            passive_fall: h.passive_fall,
        }
    }
}

impl From<YamlBackend> for Backend {
    fn from(y: YamlBackend) -> Self {
        Backend {
            address: y.address,
            weight: y.weight,
            name: y.name,
            probe_qname: y.probe_qname,
            probe_qtype: y.probe_qtype,
            probe_source: y.probe_source,
            transport: y.transport,
            remove: y.remove.filter(|r| *r),
        }
    }
}

impl From<YamlControlTls> for ControlTlsConfig {
    fn from(y: YamlControlTls) -> Self {
        ControlTlsConfig {
            cert_path: y.cert_path,
            key_path: y.key_path,
            client_ca_path: y.client_ca_path.unwrap_or_default(),
        }
    }
}

impl From<YamlControl> for ControlConfig {
    fn from(y: YamlControl) -> Self {
        ControlConfig {
            listen_address: y.listen_address,
            reflection_enabled: y.reflection_enabled,
            api_keys: y.api_keys,
            tls: y.tls.map(Into::into),
        }
    }
}

impl From<YamlLookup> for LookupConfig {
    fn from(y: YamlLookup) -> Self {
        LookupConfig {
            profiles: y
                .profiles
                .into_iter()
                .map(|(name, profile)| (name, profile.into()))
                .collect(),
        }
    }
}

impl From<YamlLookupProfile> for LookupProfile {
    fn from(y: YamlLookupProfile) -> Self {
        LookupProfile {
            providers: y.providers.into_iter().map(Into::into).collect(),
        }
    }
}

impl From<YamlLookupProvider> for LookupProvider {
    fn from(y: YamlLookupProvider) -> Self {
        LookupProvider {
            r#type: y.provider_type,
            cache: y.cache,
        }
    }
}

impl From<YamlCacheInstance> for CacheInstance {
    fn from(y: YamlCacheInstance) -> Self {
        cache_instance_from_yaml(y).expect("cache instance without invalid lmdb.map_size")
    }
}

impl From<YamlCacheNegativeConfig> for CacheNegativeConfig {
    fn from(y: YamlCacheNegativeConfig) -> Self {
        CacheNegativeConfig {
            enabled: y.enabled,
            nxdomain_covers_descendants: y.nxdomain_covers_descendants,
            servfail_ttl_secs: y.servfail_ttl_secs,
        }
    }
}

impl From<YamlCacheTruncatedUdpConfig> for CacheTruncatedUdpConfig {
    fn from(y: YamlCacheTruncatedUdpConfig) -> Self {
        CacheTruncatedUdpConfig {
            enabled: y.enabled,
            ttl_secs: y.ttl_secs,
        }
    }
}

impl From<YamlCacheOnHitConfig> for CacheOnHitConfig {
    fn from(y: YamlCacheOnHitConfig) -> Self {
        CacheOnHitConfig {
            response_rules: y.response_rules,
        }
    }
}

impl From<YamlCacheMemoryConfig> for CacheMemoryConfig {
    fn from(y: YamlCacheMemoryConfig) -> Self {
        CacheMemoryConfig {
            shard_count: y.shard_count,
            eviction: y.eviction,
        }
    }
}

impl From<&CacheLmdbConfig> for YamlCacheLmdbConfig {
    fn from(c: &CacheLmdbConfig) -> Self {
        YamlCacheLmdbConfig {
            path: c.path.clone(),
            // Export as bare bytes; operators may rewrite as SI in source YAML.
            map_size: YamlMapSize::Bytes(c.map_size_bytes),
            when_full: c.when_full.clone(),
            sample_size: c.sample_size,
            shard_count: c.shard_count,
            sync: c.sync.clone(),
            sync_interval: c.sync_interval.clone(),
        }
    }
}

impl From<YamlCacheKeyConfig> for CacheKeyConfig {
    fn from(y: YamlCacheKeyConfig) -> Self {
        CacheKeyConfig {
            augment: y.augment.map(|_| CacheKeyAugmentConfig {}),
        }
    }
}

/// Build the YAML view of `cfg` for serialization. Default sections are omitted when sparse.
pub(crate) fn config_to_yaml(cfg: &Config) -> Result<YamlConfig, ConfigError> {
    Ok(YamlConfig {
        schema_version: cfg.schema_version,
        listeners: cfg
            .listeners
            .as_ref()
            .map(TryInto::try_into)
            .transpose()?
            .unwrap_or_default(),
        forward: cfg
            .forward
            .as_ref()
            .map(TryInto::try_into)
            .transpose()?
            .unwrap_or_default(),
        orchestrator: cfg
            .orchestrator
            .as_ref()
            .map(TryInto::try_into)
            .transpose()?
            .unwrap_or_default(),
        events: cfg
            .events
            .as_ref()
            .map(TryInto::try_into)
            .transpose()?
            .unwrap_or_default(),
        rhai: cfg
            .rhai
            .as_ref()
            .map(TryInto::try_into)
            .transpose()?
            .unwrap_or_default(),
        pools: cfg
            .pools
            .iter()
            .map(|p| p.try_into())
            .collect::<Result<_, _>>()?,
        control: cfg.control.as_ref().map(TryInto::try_into).transpose()?,
        rules: cfg
            .rules
            .as_ref()
            .map(|r| r.try_into())
            .transpose()?
            .unwrap_or_default(),
        logging: cfg
            .logging
            .as_ref()
            .map(YamlLogging::from)
            .unwrap_or_default(),
        data_sources: cfg.data_sources.iter().map(YamlDataSource::from).collect(),
        metrics: cfg.metrics.as_ref().map(YamlMetrics::from),
        tracing: cfg.tracing.as_ref().map(YamlTracing::from),
        dataplane: cfg.dataplane.as_ref().and_then(|d| {
            let y = YamlDataplane {
                runtime: d.runtime.clone(),
                policy_workers: d.policy_workers,
                io_workers: d.io_workers,
                slot_chunk_size: d.slot_chunk_size,
            };
            if y.is_sync_default() {
                None
            } else {
                Some(y)
            }
        }),
        shutdown: cfg.shutdown.as_ref().and_then(|s| {
            let y = YamlShutdown {
                drain: s.drain,
                drain_timeout_ms: s.drain_timeout_ms,
            };
            if y.is_default() {
                None
            } else {
                Some(y)
            }
        }),
        data_source_limits: cfg.data_source_limits.as_ref().and_then(|l| {
            let y = YamlDataSourceLimits::from(l);
            if y.is_default() {
                None
            } else {
                Some(y)
            }
        }),
        caches: cfg.caches.iter().map(YamlCacheInstance::from).collect(),
        lookup: cfg.lookup.as_ref().and_then(|l| {
            let y = YamlLookup::from(l);
            if y.is_empty() {
                None
            } else {
                Some(y)
            }
        }),
        acls: cfg.acls.as_ref().map(YamlAcls::from),
    })
}

impl From<&MetricsConfig> for YamlMetrics {
    fn from(m: &MetricsConfig) -> Self {
        YamlMetrics {
            enabled: m.enabled,
            profile: m.profile.clone(),
            prometheus: m.prometheus.as_ref().map(YamlPrometheusMetrics::from),
            otel: m.otel.as_ref().map(YamlOtelMetrics::from),
            user_metrics: m
                .user_metrics
                .iter()
                .map(|u| YamlUserMetricExport {
                    name: u.name.clone(),
                    export: u.export.clone(),
                    collect: u.collect,
                    emit: u.emit,
                    help: u.help.clone(),
                })
                .collect(),
            base: m.base.clone(),
            categories: m.categories.as_ref().map(|c| YamlMetricsCategories {
                include: if c.include_set {
                    Some(c.include.clone())
                } else {
                    None
                },
                exclude: if c.exclude_set {
                    Some(c.exclude.clone())
                } else {
                    None
                },
            }),
            granularity: m.granularity.as_ref().map(|g| YamlGranularity {
                default: g.default.clone(),
                overrides: g
                    .overrides
                    .iter()
                    .map(|(family, list)| (family.clone(), YamlFamilyGranularity::from_proto(list)))
                    .collect(),
            }),
            collection: m
                .collection
                .iter()
                .map(|(name, ce)| {
                    (
                        name.clone(),
                        YamlCollectEmit {
                            collect: ce.collect,
                            emit: ce.emit,
                        },
                    )
                })
                .collect(),
            event_export: m.event_export.as_ref().map(|ce| YamlCollectEmit {
                collect: ce.collect,
                emit: ce.emit,
            }),
        }
    }
}

impl From<&PrometheusMetricsConfig> for YamlPrometheusMetrics {
    fn from(p: &PrometheusMetricsConfig) -> Self {
        YamlPrometheusMetrics {
            listen_address: p.listen_address.clone(),
            path: p.path.clone(),
        }
    }
}

impl From<&OtelMetricsConfig> for YamlOtelMetrics {
    fn from(o: &OtelMetricsConfig) -> Self {
        YamlOtelMetrics {
            endpoint: o.endpoint.clone(),
            push_interval_ms: if o.push_interval_ms == 0 {
                None
            } else {
                Some(o.push_interval_ms)
            },
            resource_attributes: o.resource_attributes.clone(),
            allow_invalid_certs: o.allow_invalid_certs,
            headers: o.headers.clone(),
        }
    }
}

impl From<&TracingConfig> for YamlTracing {
    fn from(t: &TracingConfig) -> Self {
        YamlTracing {
            enabled: t.enabled,
            activation: t
                .activation
                .as_ref()
                .map(YamlTracingActivation::from)
                .unwrap_or_default(),
            output: t
                .output
                .as_ref()
                .map(YamlTracingOutput::from)
                .unwrap_or_default(),
        }
    }
}

impl From<&TracingActivation> for YamlTracingActivation {
    fn from(a: &TracingActivation) -> Self {
        YamlTracingActivation {
            tag: a.tag.clone(),
            selectors: a.selectors.iter().map(YamlSelector::from).collect(),
            sample_percent: a.sample_percent,
            sample_key: a.sample_key.clone(),
            sample_key_from: a.sample_key_from.clone(),
        }
    }
}

impl From<&TracingOutput> for YamlTracingOutput {
    fn from(o: &TracingOutput) -> Self {
        YamlTracingOutput {
            log_json: o.log_json,
        }
    }
}

impl From<&LoggingConfig> for YamlLogging {
    fn from(l: &LoggingConfig) -> Self {
        YamlLogging {
            level: if l.level.is_empty() {
                default_log_level()
            } else {
                l.level.clone()
            },
            output: if l.output.is_empty() {
                default_log_output()
            } else {
                l.output.clone()
            },
            query_access: l.query_access.as_ref().map(YamlQueryAccess::from),
        }
    }
}

impl From<&QueryAccessLogging> for YamlQueryAccess {
    fn from(q: &QueryAccessLogging) -> Self {
        YamlQueryAccess {
            acl_denied: q.acl_denied.clone(),
            acl_denied_sample: q.acl_denied_sample.as_ref().map(YamlAclDeniedSample::from),
        }
    }
}

impl From<&AclDeniedSample> for YamlAclDeniedSample {
    fn from(s: &AclDeniedSample) -> Self {
        YamlAclDeniedSample {
            mode: s.mode.clone(),
            rate: s.rate,
            nth: s.nth,
        }
    }
}

impl From<&AclsConfig> for YamlAcls {
    fn from(a: &AclsConfig) -> Self {
        YamlAcls {
            default_action: if a.default_action.is_empty() {
                default_acl_default_action()
            } else {
                a.default_action.clone()
            },
            rules: a.rules.iter().map(YamlAclRule::from).collect(),
        }
    }
}

impl From<&AclRule> for YamlAclRule {
    fn from(r: &AclRule) -> Self {
        YamlAclRule {
            match_view: r.r#match.clone(),
            action: r.action.clone(),
            tag: r.tag.clone(),
        }
    }
}

impl TryFrom<&RulesConfig> for YamlRules {
    type Error = ConfigError;

    fn try_from(r: &RulesConfig) -> Result<Self, Self::Error> {
        Ok(YamlRules {
            match_mode: r.match_mode.clone(),
            rules: r
                .rules
                .iter()
                .map(YamlRule::try_from)
                .collect::<Result<_, _>>()?,
        })
    }
}

impl TryFrom<&Rule> for YamlRule {
    type Error = ConfigError;

    fn try_from(rule: &Rule) -> Result<Self, Self::Error> {
        Ok(YamlRule {
            name: rule.name.clone(),
            hook: rule.hook.clone(),
            selectors: rule.selectors.iter().map(YamlSelector::from).collect(),
            actions: rule.actions.iter().map(YamlAction::from).collect(),
        })
    }
}

impl From<&Selector> for YamlSelector {
    fn from(s: &Selector) -> Self {
        YamlSelector {
            selector_type: s.r#type.clone(),
            value: s.value.clone(),
            key: s.key.clone(),
            key_from: s.key_from.clone(),
        }
    }
}

impl From<&Action> for YamlAction {
    fn from(a: &Action) -> Self {
        YamlAction {
            action_type: a.r#type.clone(),
            value: a.value.clone(),
        }
    }
}

impl TryFrom<&ListenersConfig> for YamlListeners {
    type Error = ConfigError;

    fn try_from(l: &ListenersConfig) -> Result<Self, Self::Error> {
        Ok(YamlListeners {
            threads: l.threads,
            reuse_port: l.reuse_port,
            rcvbuf: l.rcvbuf,
            sndbuf: l.sndbuf,
            listeners: l.listeners.iter().map(YamlListener::from).collect(),
        })
    }
}

impl From<&Listener> for YamlListener {
    fn from(ln: &Listener) -> Self {
        YamlListener {
            address: ln.address.clone(),
            protocol: ln.protocol.clone(),
            threads: ln.threads,
            reuse_port: ln.reuse_port,
            name: ln.name.clone(),
            rcvbuf: ln.rcvbuf,
            acls: ln.acls.as_ref().map(YamlAcls::from),
        }
    }
}

impl TryFrom<&ForwardConfig> for YamlForward {
    type Error = ConfigError;

    fn try_from(f: &ForwardConfig) -> Result<Self, Self::Error> {
        Ok(YamlForward {
            outstanding_per_backend: f.outstanding_per_backend,
            timeout_ms: f.timeout_ms,
            sources_v4: f.sources_v4.clone(),
            source_selection: if f.source_selection.is_empty() {
                default_source_selection()
            } else {
                f.source_selection.clone()
            },
            sources_v6: f.sources_v6.clone(),
            upstream_transport: f.upstream_transport.clone(),
            client_tcp_uses_upstream_tcp: f.client_tcp_uses_upstream_tcp,
        })
    }
}

impl TryFrom<&OrchestratorConfig> for YamlOrchestrator {
    type Error = ConfigError;

    fn try_from(o: &OrchestratorConfig) -> Result<Self, Self::Error> {
        Ok(YamlOrchestrator {
            max_attempts: o.max_attempts,
            max_txn_duration_ms: o.max_txn_duration_ms,
            txn_table_capacity: o.txn_table_capacity,
        })
    }
}

impl TryFrom<&EventsConfig> for YamlEvents {
    type Error = ConfigError;

    fn try_from(o: &EventsConfig) -> Result<Self, Self::Error> {
        Ok(YamlEvents {
            queue_depth: o.queue_depth,
            drop_policy: o.drop_policy.clone(),
            sinks: o.sinks.iter().map(YamlEventSink::from).collect(),
        })
    }
}

impl From<&conduit_proto::config::EventSink> for YamlEventSink {
    fn from(s: &conduit_proto::config::EventSink) -> Self {
        let (name, export_id) = match conduit_events::resolve_sink_identity(s) {
            Ok(identity) if identity.name == identity.export_id => {
                (Some(identity.name), String::new())
            }
            Ok(identity) => (Some(identity.name), identity.export_id),
            Err(_) => (s.name.clone(), s.export_id.clone()),
        };
        YamlEventSink {
            sink_type: s.r#type.clone(),
            export_id,
            name,
            destinations: s.destinations.clone(),
            emit: s.emit.clone(),
            filters: s
                .filters
                .as_ref()
                .map(|f| YamlEventSinkFilters {
                    tag_required: f.tag_required.clone(),
                    selectors: f.selectors.iter().map(YamlSelector::from).collect(),
                    sample_percent: f.sample_percent,
                    sample_key: f.sample_key.clone(),
                    sample_key_from: f.sample_key_from.clone(),
                    pool: f.pool.clone(),
                    backend: f.backend.clone(),
                })
                .unwrap_or_default(),
            extra_fields: s.extra_fields.clone(),
            extra_tags: s.extra_tags.clone(),
            connect_retry: s.connect_retry.as_ref().map(|r| YamlConnectRetry {
                initial_ms: r.initial_ms,
                max_ms: r.max_ms,
                multiplier: r.multiplier,
                max_elapsed_ms: r.max_elapsed_ms,
                jitter: r.jitter,
            }),
        }
    }
}

impl TryFrom<&RhaiConfig> for YamlRhai {
    type Error = ConfigError;

    fn try_from(r: &RhaiConfig) -> Result<Self, Self::Error> {
        Ok(YamlRhai {
            max_operations: r.max_operations,
            max_call_depth: r.max_call_depth,
            hook_timeout_ms: r.hook_timeout_ms,
        })
    }
}

impl From<&DataSource> for YamlDataSource {
    fn from(d: &DataSource) -> Self {
        YamlDataSource {
            name: d.name.clone(),
            source_type: d.r#type.clone(),
            path: d.path.clone(),
            key_column: d.key_column.clone(),
            value_column: d.value_column.clone(),
            max_file_bytes: d.max_file_bytes,
            max_entries: d.max_entries,
            max_key_bytes: d.max_key_bytes,
            max_value_bytes: d.max_value_bytes,
        }
    }
}

impl TryFrom<&Pool> for YamlPool {
    type Error = ConfigError;

    fn try_from(p: &Pool) -> Result<Self, Self::Error> {
        Ok(YamlPool {
            name: p.name.clone(),
            backends: p.backends.iter().map(YamlBackend::from).collect(),
            sources_v4: p.sources_v4.clone(),
            sources_v6: p.sources_v6.clone(),
            max_inflight: p.max_inflight,
            health: p.health.as_ref().map(YamlHealthCheck::from),
            // Effective/export must never emit remove markers.
            remove: None,
        })
    }
}

impl From<&Backend> for YamlBackend {
    fn from(b: &Backend) -> Self {
        YamlBackend {
            address: b.address.clone(),
            name: b.name.clone(),
            weight: match b.weight {
                None | Some(DEFAULT_BACKEND_WEIGHT) => None,
                Some(w) => Some(w),
            },
            probe_qname: b.probe_qname.clone(),
            probe_qtype: b.probe_qtype.clone(),
            probe_source: b.probe_source.clone(),
            transport: b.transport.clone(),
            // Effective/export must never emit remove markers.
            remove: None,
        }
    }
}

impl TryFrom<&ControlConfig> for YamlControl {
    type Error = ConfigError;

    fn try_from(c: &ControlConfig) -> Result<Self, Self::Error> {
        Ok(YamlControl {
            listen_address: c.listen_address.clone(),
            reflection_enabled: c.reflection_enabled,
            api_keys: c.api_keys.clone(),
            tls: c.tls.as_ref().map(|t| YamlControlTls {
                cert_path: t.cert_path.clone(),
                key_path: t.key_path.clone(),
                client_ca_path: if t.client_ca_path.is_empty() {
                    None
                } else {
                    Some(t.client_ca_path.clone())
                },
            }),
        })
    }
}

impl From<&LookupConfig> for YamlLookup {
    fn from(l: &LookupConfig) -> Self {
        YamlLookup {
            profiles: l
                .profiles
                .iter()
                .map(|(name, profile)| (name.clone(), YamlLookupProfile::from(profile)))
                .collect(),
        }
    }
}

impl From<&LookupProfile> for YamlLookupProfile {
    fn from(p: &LookupProfile) -> Self {
        YamlLookupProfile {
            providers: p.providers.iter().map(YamlLookupProvider::from).collect(),
        }
    }
}

impl From<&LookupProvider> for YamlLookupProvider {
    fn from(p: &LookupProvider) -> Self {
        YamlLookupProvider {
            provider_type: p.r#type.clone(),
            cache: p.cache.clone(),
        }
    }
}

impl From<&CacheInstance> for YamlCacheInstance {
    fn from(c: &CacheInstance) -> Self {
        YamlCacheInstance {
            name: c.name.clone(),
            cache_type: c.r#type.clone(),
            negative_cache: c.negative_cache.as_ref().map(YamlCacheNegativeConfig::from),
            on_hit: c.on_hit.as_ref().map(YamlCacheOnHitConfig::from),
            truncated_udp: c
                .truncated_udp
                .as_ref()
                .map(YamlCacheTruncatedUdpConfig::from),
            rotate_rrset_on_serve: c.rotate_rrset_on_serve,
            memory: c.memory.as_ref().map(YamlCacheMemoryConfig::from),
            lmdb: c.lmdb.as_ref().map(YamlCacheLmdbConfig::from),
            key: c.key.as_ref().map(YamlCacheKeyConfig::from),
            max_entries: c.max_entries,
        }
    }
}

impl From<&CacheNegativeConfig> for YamlCacheNegativeConfig {
    fn from(n: &CacheNegativeConfig) -> Self {
        YamlCacheNegativeConfig {
            enabled: n.enabled,
            nxdomain_covers_descendants: n.nxdomain_covers_descendants,
            servfail_ttl_secs: n.servfail_ttl_secs,
        }
    }
}

impl From<&CacheTruncatedUdpConfig> for YamlCacheTruncatedUdpConfig {
    fn from(t: &CacheTruncatedUdpConfig) -> Self {
        YamlCacheTruncatedUdpConfig {
            enabled: t.enabled,
            ttl_secs: t.ttl_secs,
        }
    }
}

impl From<&CacheOnHitConfig> for YamlCacheOnHitConfig {
    fn from(o: &CacheOnHitConfig) -> Self {
        YamlCacheOnHitConfig {
            response_rules: o.response_rules.clone(),
        }
    }
}

impl From<&CacheMemoryConfig> for YamlCacheMemoryConfig {
    fn from(m: &CacheMemoryConfig) -> Self {
        YamlCacheMemoryConfig {
            shard_count: m.shard_count,
            eviction: m.eviction.clone(),
        }
    }
}

impl From<&CacheKeyConfig> for YamlCacheKeyConfig {
    fn from(k: &CacheKeyConfig) -> Self {
        YamlCacheKeyConfig {
            augment: k.augment.as_ref().map(|_| YamlCacheKeyAugmentConfig {}),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::defaults::{
        DEFAULT_EVENTS_DROP_POLICY, DEFAULT_EVENTS_QUEUE_DEPTH,
        DEFAULT_FORWARD_OUTSTANDING_PER_BACKEND, DEFAULT_FORWARD_TIMEOUT_MS,
        DEFAULT_LISTENER_THREADS, DEFAULT_ORCHESTRATOR_MAX_ATTEMPTS,
        DEFAULT_ORCHESTRATOR_MAX_TXN_DURATION_MS, DEFAULT_ORCHESTRATOR_TXN_TABLE_CAPACITY,
        DEFAULT_RHAI_MAX_CALL_DEPTH, DEFAULT_RHAI_MAX_OPERATIONS,
    };
    use conduit_proto::config::Config;

    #[test]
    fn load_minimal_yaml() {
        let yaml = include_str!("../../../tests/fixtures/config/minimal.yaml");
        let cfg: Config = load_yaml(yaml).expect("parse");
        assert_eq!(cfg.schema_version, 1);
        assert_eq!(cfg.listeners.as_ref().unwrap().threads, 2);
    }

    #[test]
    fn load_minimal_sparse_yaml() {
        let yaml = include_str!("../../../tests/fixtures/config/minimal-sparse.yaml");
        let cfg: Config = load_yaml(yaml).expect("parse");
        assert_eq!(cfg.schema_version, 1);
        assert!(cfg.control.is_none());

        let forward = cfg.forward.as_ref().expect("forward defaults applied");
        assert_eq!(
            forward.outstanding_per_backend,
            DEFAULT_FORWARD_OUTSTANDING_PER_BACKEND
        );
        assert_eq!(forward.timeout_ms, DEFAULT_FORWARD_TIMEOUT_MS);

        let orchestrator = cfg.orchestrator.as_ref().expect("orchestrator defaults");
        assert_eq!(orchestrator.max_attempts, DEFAULT_ORCHESTRATOR_MAX_ATTEMPTS);
        assert_eq!(
            orchestrator.max_txn_duration_ms,
            DEFAULT_ORCHESTRATOR_MAX_TXN_DURATION_MS
        );
        assert_eq!(
            orchestrator.txn_table_capacity,
            DEFAULT_ORCHESTRATOR_TXN_TABLE_CAPACITY
        );

        let events = cfg.events.as_ref().expect("events defaults");
        assert_eq!(events.queue_depth, DEFAULT_EVENTS_QUEUE_DEPTH);
        assert_eq!(events.drop_policy, DEFAULT_EVENTS_DROP_POLICY);

        let rhai = cfg.rhai.as_ref().expect("rhai defaults");
        assert_eq!(rhai.max_operations, DEFAULT_RHAI_MAX_OPERATIONS);
        assert_eq!(rhai.max_call_depth, DEFAULT_RHAI_MAX_CALL_DEPTH);

        let listeners = cfg.listeners.as_ref().expect("listeners");
        assert_eq!(listeners.threads, DEFAULT_LISTENER_THREADS);
    }

    #[test]
    fn sparse_yaml_matches_explicit_default_sections() {
        let sparse = include_str!("../../../tests/fixtures/config/minimal-sparse.yaml");
        let sparse_cfg = load_yaml(sparse).expect("sparse");

        let explicit = format!(
            r#"
schema_version: 1
listeners:
  threads: {DEFAULT_LISTENER_THREADS}
  reuse_port: false
  listeners:
    - address: "127.0.0.1:15353"
      protocol: udp
forward:
  outstanding_per_backend: {DEFAULT_FORWARD_OUTSTANDING_PER_BACKEND}
  timeout_ms: {DEFAULT_FORWARD_TIMEOUT_MS}
orchestrator:
  max_attempts: {DEFAULT_ORCHESTRATOR_MAX_ATTEMPTS}
  max_txn_duration_ms: {DEFAULT_ORCHESTRATOR_MAX_TXN_DURATION_MS}
  txn_table_capacity: {DEFAULT_ORCHESTRATOR_TXN_TABLE_CAPACITY}
events:
  queue_depth: {DEFAULT_EVENTS_QUEUE_DEPTH}
  drop_policy: {DEFAULT_EVENTS_DROP_POLICY}
rhai:
  max_operations: {DEFAULT_RHAI_MAX_OPERATIONS}
  max_call_depth: {DEFAULT_RHAI_MAX_CALL_DEPTH}
pools:
  - name: default
    backends:
      - address: "127.0.0.1:5300"
"#
        );
        let explicit_cfg = load_yaml(&explicit).expect("explicit defaults");

        assert_eq!(sparse_cfg.forward, explicit_cfg.forward);
        assert_eq!(sparse_cfg.orchestrator, explicit_cfg.orchestrator);
        assert_eq!(sparse_cfg.events, explicit_cfg.events);
        assert_eq!(sparse_cfg.rhai, explicit_cfg.rhai);
        assert!(sparse_cfg.control.is_none());
        assert!(explicit_cfg.control.is_none());
    }

    #[test]
    fn load_backend_without_weight_defaults_in_effective_routing() {
        let yaml = include_str!("../../../tests/fixtures/config/minimal-no-weight.yaml");
        let cfg: Config = load_yaml(yaml).expect("parse");
        assert!(cfg.pools[0].backends[0].weight.is_none());
        assert_eq!(
            crate::effective_backend_weight(&cfg.pools[0].backends[0]),
            DEFAULT_BACKEND_WEIGHT
        );
    }

    #[test]
    fn load_with_rules_yaml() {
        let yaml = include_str!("../../../tests/fixtures/config/with-rules.yaml");
        let cfg: Config = load_yaml(yaml).expect("parse");
        let rules = cfg.rules.as_ref().expect("rules");
        assert_eq!(rules.rules.len(), 2);
        assert_eq!(rules.rules[0].name, "use-primary");
    }

    #[test]
    fn lookup_forward_only_fixture_validates() {
        let yaml = include_str!("../../../tests/fixtures/config/lookup-forward-only.yaml");
        let cfg = load_yaml(yaml).expect("parse");
        let v = crate::validate::validate(&cfg);
        assert!(v.ok, "{:?}", v.errors);
        let lookup = cfg.lookup.as_ref().expect("lookup");
        assert!(lookup.profiles.contains_key("default"));
    }

    #[test]
    fn lookup_cache_enabled_fixture_validates() {
        let yaml = include_str!("../../../tests/fixtures/config/lookup-cache-enabled.yaml");
        let cfg = load_yaml(yaml).expect("parse");
        let v = crate::validate::validate(&cfg);
        assert!(v.ok, "{:?}", v.errors);
        assert_eq!(cfg.caches.len(), 1);
    }

    #[test]
    fn lookup_invalid_fixtures_fail_validation() {
        let cases = [
            include_str!("../../../tests/fixtures/config/lookup-invalid-cache-ref.yaml"),
            include_str!("../../../tests/fixtures/config/lookup-invalid-on-hit.yaml"),
            include_str!("../../../tests/fixtures/config/lookup-invalid-truncated-ttl.yaml"),
        ];
        for yaml in cases {
            let cfg = load_yaml(yaml).expect("parse");
            let v = crate::validate::validate(&cfg);
            assert!(!v.ok);
        }
    }

    #[test]
    fn lookup_cache_round_trips_through_export() {
        let yaml = include_str!("../../../tests/fixtures/config/lookup-cache-enabled.yaml");
        let cfg = load_yaml(yaml).expect("parse");
        let exported = crate::export_yaml(&cfg).expect("export");
        assert!(exported.contains("lookup:"));
        assert!(exported.contains("caches:"));
        let reparsed = load_yaml(&exported).expect("reparse");
        assert_eq!(reparsed.lookup, cfg.lookup);
        assert_eq!(reparsed.caches.len(), cfg.caches.len());
    }

    #[test]
    fn shutdown_block_round_trips() {
        let yaml = r#"
schema_version: 1
listeners:
  threads: 1
  listeners:
    - address: "127.0.0.1:15353"
      protocol: udp
pools:
  - name: default
    backends:
      - address: "127.0.0.1:5300"
shutdown:
  drain: false
  drain_timeout_ms: 250
"#;
        let cfg: Config = load_yaml(yaml).expect("parse");
        let shutdown = cfg.shutdown.as_ref().expect("shutdown block present");
        assert_eq!(shutdown.drain, Some(false));
        assert_eq!(shutdown.drain_timeout_ms, Some(250));

        let exported = crate::export_yaml(&cfg).expect("export");
        assert!(exported.contains("shutdown:"));
        let reparsed = load_yaml(&exported).expect("reparse");
        assert_eq!(reparsed.shutdown, cfg.shutdown);
    }

    #[test]
    fn shutdown_block_absent_omitted_on_export() {
        let yaml = include_str!("../../../tests/fixtures/config/minimal.yaml");
        let cfg: Config = load_yaml(yaml).expect("parse");
        assert!(cfg.shutdown.is_none());
        let exported = crate::export_yaml(&cfg).expect("export");
        assert!(!exported.contains("shutdown:"));
    }

    #[test]
    fn metrics_configurability_fields_parse_and_round_trip() {
        let yaml = r#"
schema_version: 1
listeners:
  threads: 1
  listeners:
    - address: "127.0.0.1:15353"
      protocol: udp
pools:
  - name: default
    backends:
      - address: "127.0.0.1:5300"
metrics:
  enabled: true
  base: standard
  categories:
    include: [timing]
    exclude: [process]
  collection:
    timing:
      collect: true
      emit: false
  granularity:
    default: fine
    timing: [pool]
  event_export:
    collect: true
    emit: true
"#;
        let cfg: Config = load_yaml(yaml).expect("parse");
        let metrics = cfg.metrics.as_ref().expect("metrics block present");
        assert_eq!(metrics.base, "standard");
        let categories = metrics.categories.as_ref().expect("categories");
        assert_eq!(categories.include, vec!["timing".to_string()]);
        assert_eq!(categories.exclude, vec!["process".to_string()]);
        let timing_collection = metrics.collection.get("timing").expect("timing override");
        assert_eq!(timing_collection.collect, Some(true));
        assert_eq!(timing_collection.emit, Some(false));
        let granularity = metrics.granularity.as_ref().expect("granularity");
        assert_eq!(granularity.default, "fine");
        assert_eq!(
            granularity
                .overrides
                .get("timing")
                .expect("timing dims")
                .dimensions,
            vec!["pool".to_string()]
        );
        assert!(
            granularity
                .overrides
                .get("timing")
                .expect("timing dims")
                .dimensions_set
        );
        let event_export = metrics.event_export.as_ref().expect("event_export");
        assert_eq!(event_export.collect, Some(true));
        assert_eq!(event_export.emit, Some(true));

        let exported = crate::export_yaml(&cfg).expect("export");
        assert!(exported.contains("base: standard"));
        let reparsed = load_yaml(&exported).expect("reparse");
        assert_eq!(reparsed.metrics, cfg.metrics);

        let metrics_yaml = export_metrics_yaml(metrics).expect("export metrics");
        assert!(metrics_yaml.contains("base: standard"));
        assert!(metrics_yaml.contains("timing:"));
        assert!(!metrics_yaml.contains("schema_version:"));
        let from_bare = load_metrics_yaml(&metrics_yaml).expect("load bare metrics");
        assert_eq!(&from_bare, metrics);
        let wrapped = format!("metrics:\n{}", indent_yaml_block(&metrics_yaml));
        let from_wrapped = load_metrics_yaml(&wrapped).expect("load wrapped metrics");
        assert_eq!(&from_wrapped, metrics);
    }

    fn indent_yaml_block(yaml: &str) -> String {
        yaml.lines()
            .map(|line| {
                if line.is_empty() {
                    String::new()
                } else {
                    format!("  {line}")
                }
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn metrics_profile_alias_still_parses_for_1x_compat() {
        let yaml = r#"
schema_version: 1
listeners:
  threads: 1
  listeners:
    - address: "127.0.0.1:15353"
      protocol: udp
pools:
  - name: default
    backends:
      - address: "127.0.0.1:5300"
metrics:
  enabled: true
  profile: full
"#;
        let cfg: Config = load_yaml(yaml).expect("parse");
        let metrics = cfg.metrics.as_ref().expect("metrics block present");
        assert_eq!(metrics.profile, "full");
        assert!(
            metrics.base.is_empty(),
            "base unset when only profile given"
        );
    }

    #[test]
    fn metrics_granularity_responses_rcode_map_form() {
        let yaml = r#"
schema_version: 1
listeners:
  threads: 1
  listeners:
    - address: "127.0.0.1:15353"
      protocol: udp
pools:
  - name: default
    backends:
      - address: "127.0.0.1:5300"
metrics:
  enabled: true
  base: standard
  granularity:
    default: fine
    responses:
      rcode: coarse
"#;
        let cfg: Config = load_yaml(yaml).expect("parse");
        let metrics = cfg.metrics.as_ref().expect("metrics");
        let g = metrics.granularity.as_ref().expect("granularity");
        let responses = g.overrides.get("responses").expect("responses");
        assert_eq!(responses.rcode, "coarse");
        assert!(!responses.dimensions_set);
        assert!(responses.dimensions.is_empty());

        let exported = crate::export_yaml(&cfg).expect("export");
        let reparsed = load_yaml(&exported).expect("reparse");
        assert_eq!(reparsed.metrics, cfg.metrics);
    }
}
