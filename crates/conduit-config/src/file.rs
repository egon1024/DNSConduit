use crate::backend::DEFAULT_BACKEND_WEIGHT;
use crate::error::ConfigError;
use conduit_proto::config::{
    Action, Backend, Config, ControlConfig, ControlTlsConfig, DataSource, EventSinkFilters,
    EventsConfig, ForwardConfig, Listener, ListenersConfig, LoggingConfig, MetricsConfig,
    OrchestratorConfig, OtelMetricsConfig, Pool, PrometheusMetricsConfig, RhaiConfig, Rule,
    RulesConfig, Selector, TracingActivation, TracingConfig, TracingOutput,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct YamlConfig {
    schema_version: u32,
    listeners: YamlListeners,
    forward: YamlForward,
    orchestrator: YamlOrchestrator,
    events: YamlEvents,
    rhai: YamlRhai,
    pools: Vec<YamlPool>,
    control: YamlControl,
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
}

#[derive(Debug, Deserialize, Serialize)]
pub(crate) struct YamlLogging {
    #[serde(default = "default_log_level")]
    level: String,
    #[serde(default = "default_log_output")]
    output: String,
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
    id: String,
    hook: String,
    selectors: Vec<YamlSelector>,
    actions: Vec<YamlAction>,
}

#[derive(Debug, Deserialize, Serialize)]
pub(crate) struct YamlSelector {
    #[serde(rename = "type")]
    selector_type: String,
    value: String,
}

#[derive(Debug, Deserialize, Serialize)]
pub(crate) struct YamlAction {
    #[serde(rename = "type")]
    action_type: String,
    value: String,
}

#[derive(Debug, Deserialize, Serialize)]
pub(crate) struct YamlListeners {
    threads: u32,
    reuse_port: bool,
    #[serde(default)]
    rcvbuf: u32,
    #[serde(default)]
    sndbuf: u32,
    listeners: Vec<YamlListener>,
}

#[derive(Debug, Deserialize, Serialize)]
pub(crate) struct YamlListener {
    address: String,
    protocol: String,
}

fn default_source_selection() -> String {
    "round_robin".into()
}

#[derive(Debug, Deserialize, Serialize)]
pub(crate) struct YamlForward {
    outstanding_per_backend: u32,
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

#[derive(Debug, Deserialize, Serialize)]
pub(crate) struct YamlOrchestrator {
    max_attempts: u32,
    max_txn_duration_ms: u32,
    txn_table_capacity: u32,
}

#[derive(Debug, Deserialize, Serialize)]
pub(crate) struct YamlEvents {
    queue_depth: u32,
    drop_policy: String,
    #[serde(default)]
    sinks: Vec<YamlEventSink>,
}

#[derive(Debug, Deserialize, Serialize, Default)]
pub(crate) struct YamlEventSinkFilters {
    #[serde(default)]
    tag_required: Option<String>,
    #[serde(default)]
    selectors: Vec<YamlSelector>,
    #[serde(default)]
    sample_rate: Option<f64>,
    #[serde(default)]
    pool: Option<String>,
    #[serde(default)]
    backend: Option<String>,
}

fn yaml_event_filters_nonempty(f: &YamlEventSinkFilters) -> bool {
    f.tag_required.is_some()
        || !f.selectors.is_empty()
        || f.sample_rate.is_some()
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
    #[serde(default)]
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
pub(crate) struct YamlDataSource {
    name: String,
    #[serde(rename = "type")]
    source_type: String,
    path: String,
    #[serde(default)]
    key_column: String,
    #[serde(default)]
    value_column: String,
}

#[derive(Debug, Deserialize, Serialize)]
pub(crate) struct YamlRhai {
    max_operations: u64,
    max_call_depth: u32,
    #[serde(default)]
    hook_timeout_ms: u32,
}

#[derive(Debug, Deserialize, Serialize)]
pub(crate) struct YamlPool {
    name: String,
    backends: Vec<YamlBackend>,
    #[serde(default)]
    sources_v4: Vec<String>,
    #[serde(default)]
    sources_v6: Vec<String>,
}

#[derive(Debug, Deserialize, Serialize)]
pub(crate) struct YamlBackend {
    address: String,
    /// Omitted in YAML means default weight (100).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    weight: Option<u32>,
}

#[derive(Debug, Deserialize, Serialize)]
pub(crate) struct YamlMetrics {
    #[serde(default)]
    enabled: bool,
    #[serde(default = "default_metrics_profile")]
    profile: String,
    #[serde(default)]
    prometheus: Option<YamlPrometheusMetrics>,
    #[serde(default)]
    otel: Option<YamlOtelMetrics>,
}

fn default_metrics_profile() -> String {
    "full".into()
}

#[derive(Debug, Deserialize, Serialize)]
pub(crate) struct YamlPrometheusMetrics {
    listen_address: String,
    #[serde(default = "default_metrics_path")]
    path: String,
}

fn default_metrics_path() -> String {
    "/metrics".into()
}

#[derive(Debug, Deserialize, Serialize)]
pub(crate) struct YamlOtelMetrics {
    endpoint: String,
    #[serde(default = "default_otel_interval")]
    push_interval_ms: u32,
    #[serde(default)]
    resource_attributes: std::collections::HashMap<String, String>,
}

fn default_otel_interval() -> u32 {
    15_000
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
    sample_rate: Option<f64>,
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
    listen_address: String,
    #[serde(default, skip_serializing_if = "is_false")]
    reflection_enabled: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    api_keys: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    tls: Option<YamlControlTls>,
}

fn is_false(v: &bool) -> bool {
    !*v
}

pub fn load_yaml(input: &str) -> Result<Config, ConfigError> {
    let y: YamlConfig = serde_yaml::from_str(input)?;
    Ok(y.into())
}

impl From<YamlConfig> for Config {
    fn from(y: YamlConfig) -> Self {
        Config {
            schema_version: y.schema_version,
            listeners: Some(y.listeners.into()),
            forward: Some(y.forward.into()),
            orchestrator: Some(y.orchestrator.into()),
            events: Some(y.events.into()),
            rhai: Some(y.rhai.into()),
            pools: y.pools.into_iter().map(Into::into).collect(),
            control: Some(y.control.into()),
            rules: Some(y.rules.into()),
            logging: Some(y.logging.into()),
            data_sources: y.data_sources.into_iter().map(Into::into).collect(),
            metrics: y.metrics.map(Into::into),
            tracing: y.tracing.map(Into::into),
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
            push_interval_ms: y.push_interval_ms,
            resource_attributes: y.resource_attributes,
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
            sample_rate: y.sample_rate,
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
            id: y.id,
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
                    })
                    .collect(),
                sample_rate: y.filters.sample_rate,
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
        }
    }
}

impl From<YamlBackend> for Backend {
    fn from(y: YamlBackend) -> Self {
        Backend {
            address: y.address,
            weight: y.weight,
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

fn missing_section(name: &str) -> ConfigError {
    ConfigError::Incomplete(format!("missing required section: {name}"))
}

/// Build the YAML view of `cfg` for serialization. All Phase 0 sections must be present.
pub(crate) fn config_to_yaml(cfg: &Config) -> Result<YamlConfig, ConfigError> {
    Ok(YamlConfig {
        schema_version: cfg.schema_version,
        listeners: cfg
            .listeners
            .as_ref()
            .ok_or_else(|| missing_section("listeners"))?
            .try_into()?,
        forward: cfg
            .forward
            .as_ref()
            .ok_or_else(|| missing_section("forward"))?
            .try_into()?,
        orchestrator: cfg
            .orchestrator
            .as_ref()
            .ok_or_else(|| missing_section("orchestrator"))?
            .try_into()?,
        events: cfg
            .events
            .as_ref()
            .ok_or_else(|| missing_section("events"))?
            .try_into()?,
        rhai: cfg
            .rhai
            .as_ref()
            .ok_or_else(|| missing_section("rhai"))?
            .try_into()?,
        pools: cfg
            .pools
            .iter()
            .map(|p| p.try_into())
            .collect::<Result<_, _>>()?,
        control: cfg
            .control
            .as_ref()
            .ok_or_else(|| missing_section("control"))?
            .try_into()?,
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
    })
}

impl From<&MetricsConfig> for YamlMetrics {
    fn from(m: &MetricsConfig) -> Self {
        YamlMetrics {
            enabled: m.enabled,
            profile: m.profile.clone(),
            prometheus: m.prometheus.as_ref().map(YamlPrometheusMetrics::from),
            otel: m.otel.as_ref().map(YamlOtelMetrics::from),
        }
    }
}

impl From<&PrometheusMetricsConfig> for YamlPrometheusMetrics {
    fn from(p: &PrometheusMetricsConfig) -> Self {
        YamlPrometheusMetrics {
            listen_address: p.listen_address.clone(),
            path: if p.path.is_empty() {
                default_metrics_path()
            } else {
                p.path.clone()
            },
        }
    }
}

impl From<&OtelMetricsConfig> for YamlOtelMetrics {
    fn from(o: &OtelMetricsConfig) -> Self {
        YamlOtelMetrics {
            endpoint: o.endpoint.clone(),
            push_interval_ms: o.push_interval_ms,
            resource_attributes: o.resource_attributes.clone(),
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
            sample_rate: a.sample_rate,
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
            id: rule.id.clone(),
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
        YamlEventSink {
            sink_type: s.r#type.clone(),
            export_id: s.export_id.clone(),
            name: s.name.clone(),
            destinations: s.destinations.clone(),
            emit: s.emit.clone(),
            filters: s
                .filters
                .as_ref()
                .map(|f| YamlEventSinkFilters {
                    tag_required: f.tag_required.clone(),
                    selectors: f.selectors.iter().map(YamlSelector::from).collect(),
                    sample_rate: f.sample_rate,
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
        })
    }
}

impl From<&Backend> for YamlBackend {
    fn from(b: &Backend) -> Self {
        YamlBackend {
            address: b.address.clone(),
            weight: match b.weight {
                None | Some(DEFAULT_BACKEND_WEIGHT) => None,
                Some(w) => Some(w),
            },
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

#[cfg(test)]
mod tests {
    use super::*;
    use conduit_proto::config::Config;

    #[test]
    fn load_minimal_yaml() {
        let yaml = include_str!("../../../tests/fixtures/config/minimal.yaml");
        let cfg: Config = load_yaml(yaml).expect("parse");
        assert_eq!(cfg.schema_version, 1);
        assert_eq!(cfg.listeners.as_ref().unwrap().threads, 2);
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
        assert_eq!(rules.rules[0].id, "use-primary");
    }
}
