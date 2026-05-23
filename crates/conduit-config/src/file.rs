use crate::backend::DEFAULT_BACKEND_WEIGHT;
use crate::error::ConfigError;
use conduit_proto::config::{
    Action, Backend, Config, ControlConfig, ForwardConfig, Listener, ListenersConfig,
    LoggingConfig, ObservationConfig, OrchestratorConfig, Pool, RhaiConfig, Rule, RulesConfig,
    Selector,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Serialize)]
pub(crate) struct YamlConfig {
    schema_version: u32,
    listeners: YamlListeners,
    forward: YamlForward,
    orchestrator: YamlOrchestrator,
    observation: YamlObservation,
    rhai: YamlRhai,
    pools: Vec<YamlPool>,
    control: YamlControl,
    #[serde(default)]
    rules: YamlRules,
    #[serde(default)]
    logging: YamlLogging,
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

#[derive(Debug, Deserialize, Serialize)]
pub(crate) struct YamlForward {
    outstanding_per_backend: u32,
    timeout_ms: u32,
}

#[derive(Debug, Deserialize, Serialize)]
pub(crate) struct YamlOrchestrator {
    max_attempts: u32,
    max_txn_duration_ms: u32,
    txn_table_capacity: u32,
}

#[derive(Debug, Deserialize, Serialize)]
pub(crate) struct YamlObservation {
    queue_depth: u32,
    drop_policy: String,
    #[serde(default)]
    sinks: Vec<YamlObservationSink>,
}

#[derive(Debug, Deserialize, Serialize, Default)]
pub(crate) struct YamlObservationSinkFilters {
    #[serde(default)]
    tag_required: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
pub(crate) struct YamlObservationSink {
    #[serde(rename = "type")]
    sink_type: String,
    #[serde(default)]
    export_id: String,
    #[serde(default)]
    destinations: Vec<String>,
    #[serde(default)]
    emit: Vec<String>,
    #[serde(default)]
    filters: YamlObservationSinkFilters,
}

#[derive(Debug, Deserialize, Serialize)]
pub(crate) struct YamlRhai {
    max_operations: u64,
    max_call_depth: u32,
}

#[derive(Debug, Deserialize, Serialize)]
pub(crate) struct YamlPool {
    name: String,
    backends: Vec<YamlBackend>,
}

#[derive(Debug, Deserialize, Serialize)]
pub(crate) struct YamlBackend {
    address: String,
    /// Omitted in YAML means default weight (100).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    weight: Option<u32>,
}

#[derive(Debug, Deserialize, Serialize)]
pub(crate) struct YamlControl {
    listen_address: String,
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
            observation: Some(y.observation.into()),
            rhai: Some(y.rhai.into()),
            pools: y.pools.into_iter().map(Into::into).collect(),
            control: Some(y.control.into()),
            rules: Some(y.rules.into()),
            logging: Some(y.logging.into()),
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

impl From<YamlObservation> for ObservationConfig {
    fn from(y: YamlObservation) -> Self {
        ObservationConfig {
            queue_depth: y.queue_depth,
            drop_policy: y.drop_policy,
            sinks: y.sinks.into_iter().map(Into::into).collect(),
        }
    }
}

impl From<YamlObservationSink> for conduit_proto::config::ObservationSink {
    fn from(y: YamlObservationSink) -> Self {
        let filters = if y.filters.tag_required.is_some() {
            Some(conduit_proto::config::ObservationSinkFilters {
                tag_required: y.filters.tag_required,
            })
        } else {
            None
        };
        conduit_proto::config::ObservationSink {
            r#type: y.sink_type,
            export_id: y.export_id,
            destinations: y.destinations,
            emit: y.emit,
            filters,
        }
    }
}

impl From<YamlRhai> for RhaiConfig {
    fn from(y: YamlRhai) -> Self {
        RhaiConfig {
            max_operations: y.max_operations,
            max_call_depth: y.max_call_depth,
        }
    }
}

impl From<YamlPool> for Pool {
    fn from(y: YamlPool) -> Self {
        Pool {
            name: y.name,
            backends: y.backends.into_iter().map(Into::into).collect(),
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

impl From<YamlControl> for ControlConfig {
    fn from(y: YamlControl) -> Self {
        ControlConfig {
            listen_address: y.listen_address,
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
        observation: cfg
            .observation
            .as_ref()
            .ok_or_else(|| missing_section("observation"))?
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
    })
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

impl TryFrom<&ObservationConfig> for YamlObservation {
    type Error = ConfigError;

    fn try_from(o: &ObservationConfig) -> Result<Self, Self::Error> {
        Ok(YamlObservation {
            queue_depth: o.queue_depth,
            drop_policy: o.drop_policy.clone(),
            sinks: o.sinks.iter().map(YamlObservationSink::from).collect(),
        })
    }
}

impl From<&conduit_proto::config::ObservationSink> for YamlObservationSink {
    fn from(s: &conduit_proto::config::ObservationSink) -> Self {
        YamlObservationSink {
            sink_type: s.r#type.clone(),
            export_id: s.export_id.clone(),
            destinations: s.destinations.clone(),
            emit: s.emit.clone(),
            filters: YamlObservationSinkFilters {
                tag_required: s.filters.as_ref().and_then(|f| f.tag_required.clone()),
            },
        }
    }
}

impl TryFrom<&RhaiConfig> for YamlRhai {
    type Error = ConfigError;

    fn try_from(r: &RhaiConfig) -> Result<Self, Self::Error> {
        Ok(YamlRhai {
            max_operations: r.max_operations,
            max_call_depth: r.max_call_depth,
        })
    }
}

impl TryFrom<&Pool> for YamlPool {
    type Error = ConfigError;

    fn try_from(p: &Pool) -> Result<Self, Self::Error> {
        Ok(YamlPool {
            name: p.name.clone(),
            backends: p.backends.iter().map(YamlBackend::from).collect(),
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
