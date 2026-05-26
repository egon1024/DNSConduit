//! Immutable runtime configuration snapshot and atomic swap (spec §4.4).

use crate::rules::CompiledRules;
use arc_swap::ArcSwap;
use conduit_config::validate;
use conduit_observation::{compile_from_config, CompiledObservation};
use conduit_proto::config::Config;
use conduit_script::{compile_from_config as compile_scripts, CompiledScripting, ScriptError};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

#[derive(Clone)]
pub struct RuntimeSnapshot {
    pub config: Config,
    pub rules: CompiledRules,
    pub observation: CompiledObservation,
    pub scripting: Arc<CompiledScripting>,
    pub generation: u64,
}

impl RuntimeSnapshot {
    pub fn from_config(config: Config) -> Self {
        Self::from_config_with_base(config, None)
    }

    pub fn from_config_with_base(config: Config, base_dir: Option<&Path>) -> Self {
        let observation = compile_from_config(&config);
        let scripting = compile_scripts(&config, base_dir).unwrap_or_else(|e| {
            panic!("script compile failed at snapshot build: {e}");
        });
        Self {
            rules: CompiledRules::compile(config.rules.as_ref()),
            observation,
            scripting: Arc::new(scripting),
            config,
            generation: 0,
        }
    }

    /// Build snapshot without panicking — for tests that expect compile errors.
    pub fn try_from_config_with_base(
        config: Config,
        base_dir: Option<&Path>,
    ) -> Result<Self, ScriptError> {
        let observation = compile_from_config(&config);
        let scripting = compile_scripts(&config, base_dir)?;
        Ok(Self {
            rules: CompiledRules::compile(config.rules.as_ref()),
            observation,
            scripting: Arc::new(scripting),
            config,
            generation: 0,
        })
    }

    pub fn observation_enabled(&self) -> bool {
        self.observation.enabled
    }

    pub fn scripting_enabled(&self) -> bool {
        !self.scripting.is_empty()
    }
}

pub struct SnapshotStore {
    current: ArcSwap<RuntimeSnapshot>,
    generation: AtomicU64,
}

impl SnapshotStore {
    pub fn new(snapshot: RuntimeSnapshot) -> Self {
        Self {
            current: ArcSwap::from_pointee(snapshot),
            generation: AtomicU64::new(0),
        }
    }

    pub fn load(&self) -> Arc<RuntimeSnapshot> {
        self.current.load_full()
    }

    pub fn generation(&self) -> u64 {
        self.generation.load(Ordering::Relaxed)
    }

    /// Replace the current snapshot and bump generation. Returns the previous snapshot.
    pub fn swap(&self, snapshot: RuntimeSnapshot) -> Arc<RuntimeSnapshot> {
        let prev = self.current.swap(Arc::new(snapshot));
        self.generation.fetch_add(1, Ordering::Relaxed);
        prev
    }

    /// Validate `cfg` and swap only if valid; on failure returns errors and leaves the store unchanged.
    pub fn install_validated(&self, cfg: Config) -> Result<(), Vec<String>> {
        let result = validate(&cfg);
        if !result.ok {
            return Err(result.errors);
        }
        let mut snap = RuntimeSnapshot::from_config(cfg);
        snap.generation = self.generation() + 1;
        self.swap(snap);
        Ok(())
    }

    pub fn install_validated_with_base(
        &self,
        cfg: Config,
        base_dir: Option<&Path>,
    ) -> Result<(), Vec<String>> {
        let result = validate(&cfg);
        if !result.ok {
            return Err(result.errors);
        }
        let mut snap = RuntimeSnapshot::from_config_with_base(cfg, base_dir);
        snap.generation = self.generation() + 1;
        self.swap(snap);
        Ok(())
    }
}

impl SnapshotStore {
    /// Build snapshot with generation aligned to store counter.
    pub fn build_snapshot(&self, config: Config) -> RuntimeSnapshot {
        let mut snap = RuntimeSnapshot::from_config(config);
        snap.generation = self.generation();
        snap
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use conduit_config::load_yaml;

    #[test]
    fn swap_snapshot_updates_generation() {
        let yaml = include_str!("../../../tests/fixtures/config/minimal.yaml");
        let cfg = load_yaml(yaml).unwrap();
        assert!(conduit_config::validate(&cfg).ok);
        let store = SnapshotStore::new(RuntimeSnapshot::from_config(cfg.clone()));
        let gen0 = store.generation();
        let mut cfg2 = cfg;
        cfg2.listeners.as_mut().unwrap().threads = 4;
        store.swap(RuntimeSnapshot::from_config(cfg2));
        assert_eq!(store.generation(), gen0 + 1);
        assert_eq!(store.load().config.listeners.as_ref().unwrap().threads, 4);
    }

    #[test]
    fn install_validated_rejects_invalid_without_swap() {
        let yaml = include_str!("../../../tests/fixtures/config/minimal.yaml");
        let file_cfg = load_yaml(yaml).unwrap();
        let store = SnapshotStore::new(RuntimeSnapshot::from_config(file_cfg.clone()));
        let gen0 = store.generation();
        let mut bad = file_cfg;
        bad.listeners.as_mut().unwrap().threads = 0;
        let err = store.install_validated(bad).unwrap_err();
        assert!(err.iter().any(|e| e.contains("threads")));
        assert_eq!(store.generation(), gen0);
        assert_eq!(store.load().config.listeners.as_ref().unwrap().threads, 2);
    }

    #[test]
    fn observation_disabled_when_no_sinks() {
        let yaml = include_str!("../../../tests/fixtures/config/no-sinks.yaml");
        let cfg = load_yaml(yaml).unwrap();
        let snap = RuntimeSnapshot::from_config(cfg);
        assert!(!snap.observation_enabled());
    }

    #[test]
    fn observation_enabled_with_dnstap_sink() {
        let yaml = include_str!("../../../tests/fixtures/config/with-dnstap.yaml");
        let cfg = load_yaml(yaml).unwrap();
        let snap = RuntimeSnapshot::from_config(cfg);
        assert!(snap.observation_enabled());
        assert_eq!(snap.observation.sinks.len(), 1);
    }

    #[test]
    fn install_validated_swaps_on_ok() {
        let yaml = include_str!("../../../tests/fixtures/config/minimal.yaml");
        let cfg = load_yaml(yaml).unwrap();
        let store = SnapshotStore::new(RuntimeSnapshot::from_config(cfg.clone()));
        let mut cfg2 = cfg;
        cfg2.listeners.as_mut().unwrap().threads = 8;
        store.install_validated(cfg2).unwrap();
        assert_eq!(store.load().config.listeners.as_ref().unwrap().threads, 8);
        assert_eq!(store.generation(), 1);
    }
}
