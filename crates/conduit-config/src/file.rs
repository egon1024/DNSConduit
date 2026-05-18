use crate::error::ConfigError;
use conduit_proto::config::{
    Backend, Config, ControlConfig, ForwardConfig, Listener, ListenersConfig, ObservationConfig,
    OrchestratorConfig, Pool, RhaiConfig,
};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct YamlConfig {
    schema_version: u32,
    listeners: YamlListeners,
    forward: YamlForward,
    orchestrator: YamlOrchestrator,
    observation: YamlObservation,
    rhai: YamlRhai,
    pools: Vec<YamlPool>,
    control: YamlControl,
}

#[derive(Debug, Deserialize)]
struct YamlListeners {
    threads: u32,
    reuse_port: bool,
    #[serde(default)]
    rcvbuf: u32,
    #[serde(default)]
    sndbuf: u32,
    listeners: Vec<YamlListener>,
}

#[derive(Debug, Deserialize)]
struct YamlListener {
    address: String,
    protocol: String,
}

#[derive(Debug, Deserialize)]
struct YamlForward {
    outstanding_per_backend: u32,
    timeout_ms: u32,
}

#[derive(Debug, Deserialize)]
struct YamlOrchestrator {
    max_attempts: u32,
    max_txn_duration_ms: u32,
    txn_table_capacity: u32,
}

#[derive(Debug, Deserialize)]
struct YamlObservation {
    queue_depth: u32,
    drop_policy: String,
    #[serde(default)]
    sinks: Vec<YamlObservationSink>,
}

#[derive(Debug, Deserialize)]
struct YamlObservationSink {
    #[serde(rename = "type")]
    sink_type: String,
    #[serde(default)]
    export_id: String,
    #[serde(default)]
    destinations: Vec<String>,
    #[serde(default)]
    emit: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct YamlRhai {
    max_operations: u64,
    max_call_depth: u32,
}

#[derive(Debug, Deserialize)]
struct YamlPool {
    name: String,
    backends: Vec<YamlBackend>,
}

#[derive(Debug, Deserialize)]
struct YamlBackend {
    address: String,
    weight: u32,
}

#[derive(Debug, Deserialize)]
struct YamlControl {
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
        conduit_proto::config::ObservationSink {
            r#type: y.sink_type,
            export_id: y.export_id,
            destinations: y.destinations,
            emit: y.emit,
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
}
