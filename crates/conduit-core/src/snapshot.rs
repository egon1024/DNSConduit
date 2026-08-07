//! Immutable runtime configuration snapshot and atomic swap (spec §4.4).

use crate::health::HealthRegistry;
use crate::lookup::LookupCacheRegistry;
use crate::rules::CompiledRules;
use arc_swap::ArcSwap;
use conduit_config::forward::{CompiledForward, CompiledPoolForward};
use conduit_config::health::{compile_health_from_config, CompiledHealth};
use conduit_config::lookup::CompiledLookup;
use conduit_config::validate;
use conduit_events::{compile_from_config as compile_events, CompiledEvents};
use conduit_metrics::{compile_from_config as compile_metrics, CompiledMetrics, CompiledTracing};
use conduit_proto::config::Config;
use conduit_script::{compile_from_config as compile_scripts, CompiledScripting, ScriptError};
use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

#[derive(Clone)]
pub struct RuntimeSnapshot {
    pub config: Config,
    pub rules: CompiledRules,
    pub events: CompiledEvents,
    pub scripting: Arc<CompiledScripting>,
    pub forward: CompiledForward,
    pub pool_forward: HashMap<String, CompiledPoolForward>,
    /// Probe *configuration* (design §D9). Runtime probe state lives outside the
    /// snapshot in a side-table reconciled across swaps — never store mutable
    /// health state here.
    pub health: CompiledHealth,
    pub lookup: CompiledLookup,
    pub metrics: CompiledMetrics,
    pub tracing: CompiledTracing,
    pub generation: u64,
}

impl RuntimeSnapshot {
    pub fn sources_v4_for_pool(&self, pool: Option<&str>) -> &[std::net::Ipv4Addr] {
        if let Some(name) = pool {
            if let Some(pf) = self.pool_forward.get(name) {
                if let Some(ref sources) = pf.sources_v4 {
                    if !sources.is_empty() {
                        return sources.as_slice();
                    }
                }
            }
        }
        self.forward.sources_v4.as_slice()
    }

    pub fn sources_v6_for_pool(&self, pool: Option<&str>) -> &[std::net::Ipv6Addr] {
        if let Some(name) = pool {
            if let Some(pf) = self.pool_forward.get(name) {
                if let Some(ref sources) = pf.sources_v6 {
                    if !sources.is_empty() {
                        return sources.as_slice();
                    }
                }
            }
        }
        self.forward.sources_v6.as_slice()
    }

    /// Allowed IPv4 source addresses for Rhai `set_source_v4` validation (forward ∪ pool).
    pub fn allowed_sources_v4_for_pool(&self, pool: Option<&str>) -> Vec<std::net::Ipv4Addr> {
        use std::collections::HashSet;
        let mut addrs = HashSet::new();
        for a in &self.forward.sources_v4 {
            addrs.insert(*a);
        }
        if let Some(name) = pool {
            if let Some(pf) = self.pool_forward.get(name) {
                if let Some(ref sources) = pf.sources_v4 {
                    for a in sources {
                        addrs.insert(*a);
                    }
                }
            }
        }
        addrs.into_iter().collect()
    }

    /// Allowed IPv6 source addresses for Rhai `set_source_v6` validation (forward ∪ pool).
    pub fn allowed_sources_v6_for_pool(&self, pool: Option<&str>) -> Vec<std::net::Ipv6Addr> {
        use std::collections::HashSet;
        let mut addrs = HashSet::new();
        for a in &self.forward.sources_v6 {
            addrs.insert(*a);
        }
        if let Some(name) = pool {
            if let Some(pf) = self.pool_forward.get(name) {
                if let Some(ref sources) = pf.sources_v6 {
                    for a in sources {
                        addrs.insert(*a);
                    }
                }
            }
        }
        addrs.into_iter().collect()
    }

    /// Unique IPv4 addresses to bind for upstream egress (forward + all pool overrides).
    pub fn egress_bind_addresses_v4(&self) -> Vec<std::net::Ipv4Addr> {
        use std::collections::HashSet;
        let mut addrs = HashSet::new();
        for addr in &self.forward.sources_v4 {
            addrs.insert(*addr);
        }
        for pf in self.pool_forward.values() {
            if let Some(ref sources) = pf.sources_v4 {
                for addr in sources {
                    addrs.insert(*addr);
                }
            }
        }
        addrs.into_iter().collect()
    }

    /// Unique IPv6 addresses to bind for upstream egress (forward + all pool overrides).
    pub fn egress_bind_addresses_v6(&self) -> Vec<std::net::Ipv6Addr> {
        use std::collections::HashSet;
        let mut addrs = HashSet::new();
        for addr in &self.forward.sources_v6 {
            addrs.insert(*addr);
        }
        for pf in self.pool_forward.values() {
            if let Some(ref sources) = pf.sources_v6 {
                for addr in sources {
                    addrs.insert(*addr);
                }
            }
        }
        addrs.into_iter().collect()
    }
}

impl RuntimeSnapshot {
    pub fn from_config(config: Config) -> Self {
        Self::from_config_with_base(config, None)
    }

    pub fn from_config_with_base(config: Config, base_dir: Option<&Path>) -> Self {
        let events = compile_events(&config, base_dir);
        let (metrics, tracing) = compile_metrics(&config);
        let scripting = compile_scripts(&config, base_dir).unwrap_or_else(|e| {
            panic!("script compile failed at snapshot build: {e}");
        });
        let (forward, pool_forward) =
            CompiledForward::compile_from_config(&config).unwrap_or_else(|e| {
                panic!("forward compile failed at snapshot build: {e}");
            });
        let health = compile_health_from_config(&config).unwrap_or_else(|e| {
            panic!("health compile failed at snapshot build: {e}");
        });
        let lookup = CompiledLookup::compile_from_config(&config).unwrap_or_else(|e| {
            panic!("lookup compile failed at snapshot build: {e}");
        });
        Self {
            rules: CompiledRules::compile(config.rules.as_ref(), &scripting),
            events,
            scripting: Arc::new(scripting),
            forward,
            pool_forward,
            health,
            lookup,
            metrics,
            tracing,
            config,
            generation: 0,
        }
    }

    /// Build snapshot without panicking — for tests that expect compile errors.
    pub fn try_from_config_with_base(
        config: Config,
        base_dir: Option<&Path>,
    ) -> Result<Self, ScriptError> {
        let events = compile_events(&config, base_dir);
        let (metrics, tracing) = compile_metrics(&config);
        let scripting = compile_scripts(&config, base_dir)?;
        let (forward, pool_forward) =
            CompiledForward::compile_from_config(&config).map_err(|e| ScriptError::Rule {
                rule_name: "forward".into(),
                message: e,
            })?;
        let health = compile_health_from_config(&config).map_err(|e| ScriptError::Rule {
            rule_name: "health".into(),
            message: e,
        })?;
        let lookup =
            CompiledLookup::compile_from_config(&config).map_err(|e| ScriptError::Rule {
                rule_name: "lookup".into(),
                message: e,
            })?;
        Ok(Self {
            rules: CompiledRules::compile(config.rules.as_ref(), &scripting),
            events,
            scripting: Arc::new(scripting),
            forward,
            pool_forward,
            health,
            lookup,
            metrics,
            tracing,
            config,
            generation: 0,
        })
    }

    pub fn metrics_enabled(&self) -> bool {
        self.metrics.enabled
    }

    pub fn tracing_master_enabled(&self) -> bool {
        self.tracing.enabled
    }

    pub fn events_enabled(&self) -> bool {
        self.events.enabled
    }

    pub fn scripting_enabled(&self) -> bool {
        !self.scripting.is_empty()
    }

    pub fn lookup_enabled(&self) -> bool {
        self.lookup.lookup_enabled()
    }

    pub fn cache_enabled(&self) -> bool {
        self.lookup.cache_enabled()
    }
}

pub struct SnapshotStore {
    current: ArcSwap<RuntimeSnapshot>,
    generation: AtomicU64,
    /// Runtime backend-health side-table (design §D9). It lives here, beside the
    /// config snapshot, because it must **outlive** snapshot swaps: a reload
    /// rebuilds the config wholesale but health is reconciled (preserve/reset by
    /// identity), never blanket-reset. The dataplane reads this same registry for
    /// the probe loop and at Route.
    health: Arc<HealthRegistry>,
    /// Runtime DNS answer cache (outside snapshot). Entry storage survives snapshot
    /// swaps; `max_entries` and new instance names are reconciled on each swap.
    cache: Arc<LookupCacheRegistry>,
}

impl SnapshotStore {
    pub fn new(snapshot: RuntimeSnapshot) -> Self {
        let health = Arc::new(HealthRegistry::from_compiled(&snapshot.health));
        let cache = Arc::new(LookupCacheRegistry::from_snapshot(
            &snapshot.lookup.cache_instances,
        ));
        Self {
            current: ArcSwap::from_pointee(snapshot),
            generation: AtomicU64::new(0),
            health,
            cache,
        }
    }

    pub fn load(&self) -> Arc<RuntimeSnapshot> {
        self.current.load_full()
    }

    /// Shared handle to the runtime health side-table (probe loop + Route).
    pub fn health(&self) -> Arc<HealthRegistry> {
        self.health.clone()
    }

    /// Shared handle to the runtime DNS answer cache registry.
    pub fn cache(&self) -> Arc<LookupCacheRegistry> {
        self.cache.clone()
    }

    pub fn generation(&self) -> u64 {
        self.generation.load(Ordering::Relaxed)
    }

    /// Replace the current snapshot and bump generation. Returns the previous
    /// snapshot. The health side-table is reconciled against the new compiled
    /// health (preserve unchanged backends, reset new/changed ones — design §D9)
    /// so a reload never wipes hard-won health state.
    ///
    /// Cache reconcile runs **before** the snapshot pointer swaps. On failure the
    /// previous snapshot remains current (last-good); callers that need soft
    /// failure should use [`Self::try_swap`].
    pub fn swap(&self, snapshot: RuntimeSnapshot) -> Arc<RuntimeSnapshot> {
        self.try_swap(snapshot)
            .unwrap_or_else(|errors| panic!("cache reconcile failed during swap: {errors:?}"))
    }

    /// Like [`Self::swap`], but returns errors when cache reopen / map_size apply fails
    /// so apply/reload can reject without installing the new snapshot.
    pub fn try_swap(&self, snapshot: RuntimeSnapshot) -> Result<Arc<RuntimeSnapshot>, Vec<String>> {
        let new = Arc::new(snapshot);
        let prev_loaded = self.current.load_full();
        self.cache
            .reconcile(
                &prev_loaded.lookup.cache_instances,
                &new.lookup.cache_instances,
            )
            .map_err(|e| vec![e])?;
        let prev = self.current.swap(new.clone());
        self.health.reconcile(&prev.health, &new.health);
        self.generation.fetch_add(1, Ordering::Relaxed);
        Ok(prev)
    }

    /// Validate `cfg` and swap only if valid; on failure returns errors and leaves the store unchanged.
    ///
    /// Production applies must go through [`crate::configurator::ConfiguratorHandle`]; tests may call this directly.
    #[allow(dead_code)]
    pub(crate) fn install_validated(&self, cfg: Config) -> Result<(), Vec<String>> {
        let result = validate(&cfg);
        if !result.ok {
            return Err(result.errors);
        }
        let mut snap = RuntimeSnapshot::try_from_config_with_base(cfg, None)
            .map_err(|e| vec![e.to_string()])?;
        snap.generation = self.generation() + 1;
        self.try_swap(snap)?;
        Ok(())
    }

    pub(crate) fn install_validated_with_base(
        &self,
        cfg: Config,
        base_dir: Option<&Path>,
    ) -> Result<(), Vec<String>> {
        let result = validate(&cfg);
        if !result.ok {
            return Err(result.errors);
        }
        let mut snap = RuntimeSnapshot::try_from_config_with_base(cfg, base_dir)
            .map_err(|e| vec![e.to_string()])?;
        snap.generation = self.generation() + 1;
        self.try_swap(snap)?;
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
    use std::path::PathBuf;

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
        assert!(!snap.events_enabled());
    }

    #[test]
    fn events_enabled_with_dnstap_sink() {
        let yaml = include_str!("../../../tests/fixtures/config/with-dnstap.yaml");
        let cfg = load_yaml(yaml).unwrap();
        let snap = RuntimeSnapshot::from_config(cfg);
        assert!(snap.events_enabled());
        assert_eq!(snap.events.sinks.len(), 1);
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

    #[test]
    fn lookup_snapshot_compiles_from_fixtures() {
        use conduit_config::CompiledLookup;

        let forward_only = load_yaml(include_str!(
            "../../../tests/fixtures/config/lookup-forward-only.yaml"
        ))
        .unwrap();
        let compiled = CompiledLookup::compile_from_config(&forward_only).unwrap();
        assert!(compiled.lookup_enabled());
        assert!(!compiled.cache_enabled());

        let cache_enabled = load_yaml(include_str!(
            "../../../tests/fixtures/config/lookup-cache-enabled.yaml"
        ))
        .unwrap();
        let compiled = CompiledLookup::compile_from_config(&cache_enabled).unwrap();
        assert!(compiled.cache_enabled());
        assert!(compiled.cache_instances.contains_key("global"));
    }

    #[test]
    fn swap_reconciles_cache_max_entries() {
        use crate::lookup::cache::build_query_key;
        use hickory_proto::op::{Message, MessageType, Query, ResponseCode};
        use hickory_proto::rr::{Name, RecordType};
        use hickory_proto::serialize::binary::{BinEncodable, BinEncoder};

        let yaml = include_str!("../../../tests/fixtures/config/lookup-cache-enabled.yaml");
        let mut cfg = load_yaml(yaml).unwrap();
        cfg.caches[0].max_entries = Some(0);
        let store = SnapshotStore::new(RuntimeSnapshot::from_config(cfg.clone()));

        let cache = store.cache();
        for i in 0..4 {
            let qname = format!("swap-max-{i}.example.");
            let mut txn = crate::transaction::Transaction::new(
                i + 1,
                "127.0.0.1:53".parse().unwrap(),
                crate::transaction::ClientProtocol::Udp,
            );
            txn.qname = Some(qname.clone());
            txn.qtype = Some(1);
            txn.qclass = Some(1);
            let name = Name::from_utf8(&qname).unwrap();
            let mut qmsg = Message::new();
            qmsg.add_query(Query::query(name.clone(), RecordType::A));
            let mut qbuf = Vec::new();
            let mut qenc = BinEncoder::new(&mut qbuf);
            qmsg.emit(&mut qenc).unwrap();
            txn.query_wire = qbuf;
            txn.dns_id = 0x1234;
            let key = build_query_key(&txn).unwrap();
            let gate = cache.instance_gate("global", &key).unwrap();
            let mut msg = Message::new();
            msg.set_message_type(MessageType::Response);
            msg.set_response_code(ResponseCode::NXDomain);
            msg.add_query(Query::query(name, RecordType::A));
            let mut buf = Vec::new();
            let mut enc = BinEncoder::new(&mut buf);
            msg.emit(&mut enc).unwrap();
            let wire: Arc<[u8]> = buf.into();
            cache.fill_from_forward("global", &key, &gate, wire, &txn);
        }
        assert_eq!(cache.entry_count("global"), 4);

        cfg.caches[0].max_entries = Some(2);
        store.swap(RuntimeSnapshot::from_config(cfg));

        assert_eq!(cache.max_entries("global"), Some(2));
        assert_eq!(cache.entry_count("global"), 2);
    }

    #[test]
    fn install_validated_rejects_compile_error_without_swap() {
        let yaml = include_str!("../../../tests/fixtures/config/with-rhai-syntax-error.yaml");
        let cfg = load_yaml(yaml).unwrap();
        assert!(validate(&cfg).ok);
        let fixtures_base =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/config");
        let store = SnapshotStore::new(
            RuntimeSnapshot::try_from_config_with_base(
                load_yaml(include_str!("../../../tests/fixtures/config/minimal.yaml")).unwrap(),
                Some(&fixtures_base),
            )
            .unwrap(),
        );
        let gen0 = store.generation();
        let err = store
            .install_validated_with_base(cfg, Some(&fixtures_base))
            .unwrap_err();
        assert!(err.iter().any(|e| e.contains("script")));
        assert_eq!(store.generation(), gen0);
    }

    #[test]
    fn try_from_config_rejects_rhai_syntax_error() {
        let yaml = include_str!("../../../tests/fixtures/config/with-rhai-syntax-error.yaml");
        let cfg = load_yaml(yaml).unwrap();
        let fixtures_base =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/config");
        let err = match RuntimeSnapshot::try_from_config_with_base(cfg, Some(&fixtures_base)) {
            Err(e) => e.to_string(),
            Ok(_) => panic!("expected rhai compile failure"),
        };
        assert!(err.contains("script"));
    }
}
