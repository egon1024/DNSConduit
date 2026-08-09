//! Runtime cache store registry (outside snapshot; shared across workers).

use super::backend::CacheBackend;
use super::entry::{CacheEntry, EntryKind};
use super::inflight::{InFlightRole, InFlightTable};
use super::key::{
    build_key_from_parts, build_query_key, build_truncated_udp_key, CacheKey, TransportKey,
};
use super::memory::entry_from_wire;
use super::memory::{ReapBudget, ReapCursor};
use super::serve::prepare_served_arc;
use arc_swap::ArcSwap;
use conduit_config::lookup::CompiledCacheInstance;
use conduit_metrics::MetricsHub;
use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Parent qnames for RFC 8020 walks (strip leftmost label).
fn ancestor_qnames(qname: &str) -> Vec<String> {
    let trimmed = qname.trim_end_matches('.');
    if trimmed.is_empty() || trimmed == "." {
        return Vec::new();
    }
    let mut labels: Vec<&str> = trimmed.split('.').collect();
    let mut out = Vec::new();
    while labels.len() > 1 {
        labels.remove(0);
        let ancestor = labels.join(".");
        out.push(format!("{ancestor}."));
    }
    out
}

fn try_ancestor_nxdomain_hit(
    inst: &CacheInstanceRuntime,
    txn: &crate::transaction::Transaction,
    now: Instant,
) -> Option<CacheEntry> {
    let cfg = inst.config.load();
    if !cfg.negative_cache.enabled || !cfg.negative_cache.nxdomain_covers_descendants {
        return None;
    }
    let qname = txn.qname.as_deref().unwrap_or(".");
    let backend = inst.backend.load();
    for ancestor in ancestor_qnames(qname) {
        let key = build_key_from_parts(
            &ancestor,
            txn.qtype.unwrap_or(0),
            txn.qclass.unwrap_or(1),
            &txn.query_wire,
            TransportKey::Complete,
        )
        .ok()?;
        let entry = backend.get(&key, now)?;
        if entry.kind == EntryKind::NxDomain {
            return Some(entry);
        }
    }
    None
}

/// Wake parked split_io policy workers waiting on cache coalescing.
pub type CacheWaitWake = Arc<dyn Fn(u64) + Send + Sync>;

/// Outcome of a cache lookup attempt.
pub enum CacheLookupOutcome {
    Hit {
        wire: Vec<u8>,
        cache_name: String,
        skip_response_rules: bool,
    },
    Miss {
        key: CacheKey,
        gate: Arc<super::inflight::InFlightGate>,
    },
    WaitAsync {
        key: CacheKey,
    },
    Bypass,
}

pub struct CacheInstanceRuntime {
    config: ArcSwap<CompiledCacheInstance>,
    backend: ArcSwap<CacheBackend>,
    /// When true, lookups Bypass and fills skip store (map_size shrink ladder).
    rebuilding: AtomicBool,
    inflight: InFlightTable,
    /// Round-robin shard index for the next active-reaper pass.
    next_reap_shard: AtomicUsize,
}

impl CacheInstanceRuntime {
    pub fn new(config: CompiledCacheInstance) -> Self {
        match Self::try_new(config) {
            Ok(inst) => inst,
            Err(e) => {
                tracing::error!(
                    error = %e,
                    "failed to open cache backend; falling back is not possible — panicking"
                );
                panic!("failed to open cache backend: {e}");
            }
        }
    }

    pub fn try_new(config: CompiledCacheInstance) -> Result<Self, String> {
        let name = config.name.clone();
        let backend = CacheBackend::from_config(&config).map_err(|e| {
            tracing::error!(cache = %name, error = %e, "failed to open cache backend");
            format!("cache '{name}': {e}")
        })?;
        Ok(Self {
            config: ArcSwap::from_pointee(config),
            backend: ArcSwap::from_pointee(backend),
            rebuilding: AtomicBool::new(false),
            inflight: InFlightTable::new(),
            next_reap_shard: AtomicUsize::new(0),
        })
    }

    pub fn is_rebuilding(&self) -> bool {
        self.rebuilding.load(Ordering::Acquire)
    }

    fn set_rebuilding(&self, v: bool) {
        self.rebuilding.store(v, Ordering::Release);
    }

    pub fn apply_max_entries(&self, max_entries: u64) {
        self.backend.load().set_max_entries(max_entries);
        let cfg = self.config.load_full();
        if cfg.max_entries != max_entries {
            let mut next = (*cfg).clone();
            next.max_entries = max_entries;
            self.config.store(Arc::new(next));
        }
    }

    pub fn max_entries(&self) -> u64 {
        self.backend.load().max_entries()
    }

    fn swap_backend(&self, backend: CacheBackend, config: CompiledCacheInstance) {
        self.backend.store(Arc::new(backend));
        self.config.store(Arc::new(config));
    }
}

pub struct LookupCacheRegistry {
    instances: RwLock<HashMap<String, Arc<CacheInstanceRuntime>>>,
    async_coalesce: AtomicBool,
    wake: RwLock<Option<CacheWaitWake>>,
    metrics: RwLock<Option<Arc<MetricsHub>>>,
}

impl LookupCacheRegistry {
    pub fn from_snapshot(instances: &HashMap<String, CompiledCacheInstance>) -> Self {
        let mut map = HashMap::new();
        for (name, cfg) in instances {
            map.insert(
                name.clone(),
                Arc::new(CacheInstanceRuntime::new(cfg.clone())),
            );
        }
        Self {
            instances: RwLock::new(map),
            async_coalesce: AtomicBool::new(false),
            wake: RwLock::new(None),
            metrics: RwLock::new(None),
        }
    }

    pub fn set_metrics(&self, metrics: Arc<MetricsHub>) {
        *self.metrics.write() = Some(metrics);
    }

    /// Reconcile named cache instances after a snapshot swap.
    ///
    /// - Adds new instances; drops names removed from config (closes LMDB env, keeps files).
    /// - Memory: live `max_entries` updates without rebuilding shards.
    /// - LMDB: Hot-apply `when_full` / `sample_size` / `max_entries`; grow/shrink `map_size`;
    ///   warm path reopen and explicit `shard_count` layout reopen (Arc swap, no migrate);
    ///   type flip rebuilds the backend in place (single-flight table retained). Failed reopen
    ///   / grow / shrink / type rebuild returns `Err` and leaves the registry unchanged for that
    ///   apply when the failure happens in preflight (open). Grow/shrink run after successful
    ///   preflight; failure rejects the apply after those ops (caller must not install the
    ///   snapshot).
    pub fn reconcile(
        &self,
        prev: &HashMap<String, CompiledCacheInstance>,
        new: &HashMap<String, CompiledCacheInstance>,
    ) -> Result<(), String> {
        // Phase 1: preflight opens (path/type/shard-layout change and new names) without
        // mutating live backends.
        let mut pending_opens: HashMap<String, PendingBackendOpen> = HashMap::new();
        {
            let guard = self.instances.read();
            for (name, cfg) in new {
                match guard.get(name) {
                    Some(inst) => {
                        let cur_cfg = inst.config.load_full();
                        let type_or_path_change = cur_cfg.backend_type != cfg.backend_type
                            || lmdb_path(cur_cfg.as_ref()) != lmdb_path(cfg);
                        let live = inst.backend.load();
                        let shard_layout_change = !type_or_path_change
                            && lmdb_explicit_shard_reopen_needed(cfg, live.as_ref());
                        if type_or_path_change {
                            let backend = CacheBackend::from_config(cfg).map_err(|e| {
                                tracing::error!(
                                    cache = %name,
                                    error = %e,
                                    "cache replacement backend open failed; rejecting apply"
                                );
                                format!("cache '{name}': failed to open replacement backend: {e}")
                            })?;
                            pending_opens.insert(
                                name.clone(),
                                PendingBackendOpen::Replace {
                                    backend,
                                    cfg: cfg.clone(),
                                },
                            );
                        } else if shard_layout_change {
                            let (backend, operator, staging) =
                                super::lmdb::LmdbCacheBackend::open_for_shard_reopen(cfg).map_err(
                                    |e| {
                                        tracing::error!(
                                            cache = %name,
                                            error = %e,
                                            "LMDB shard_count reopen open failed; rejecting apply"
                                        );
                                        format!(
                                            "cache '{name}': failed to open replacement shard layout: {e}"
                                        )
                                    },
                                )?;
                            pending_opens.insert(
                                name.clone(),
                                PendingBackendOpen::ShardReopen {
                                    backend: CacheBackend::Lmdb(backend),
                                    cfg: cfg.clone(),
                                    operator,
                                    staging,
                                },
                            );
                        }
                    }
                    None => {
                        let backend = CacheBackend::from_config(cfg).map_err(|e| {
                            tracing::error!(
                                cache = %name,
                                error = %e,
                                "new cache backend open failed; rejecting apply"
                            );
                            format!("cache '{name}': failed to open new backend: {e}")
                        })?;
                        pending_opens.insert(
                            name.clone(),
                            PendingBackendOpen::Replace {
                                backend,
                                cfg: cfg.clone(),
                            },
                        );
                    }
                }
            }
        }

        // Phase 2: same-path Hot ops first (may fail and reject); then commit preflighted
        // opens; then drop removed names.
        let mut guard = self.instances.write();

        for (name, cfg) in new {
            if pending_opens.contains_key(name) {
                continue;
            }
            let Some(inst) = guard.get(name) else {
                continue;
            };

            if prev.get(name).map(|p| p.max_entries) != Some(cfg.max_entries) {
                inst.apply_max_entries(cfg.max_entries);
            }

            let be = inst.backend.load();
            if let (Some(lmdb_backend), Some(new_lmdb)) = (be.as_lmdb(), cfg.lmdb.as_ref()) {
                lmdb_backend.apply_policy(new_lmdb.when_full, new_lmdb.sample_size);
                let cur = lmdb_backend.map_size_bytes();
                if new_lmdb.map_size_bytes > cur {
                    lmdb_backend
                        .grow_map_size(new_lmdb.map_size_bytes)
                        .map_err(|e| {
                            tracing::error!(
                                cache = %name,
                                error = %e,
                                "LMDB map_size grow failed; rejecting apply"
                            );
                            format!("cache '{name}': LMDB map_size grow failed: {e}")
                        })?;
                    tracing::info!(
                        cache = %name,
                        from = cur,
                        to = new_lmdb.map_size_bytes,
                        "LMDB map_size grown"
                    );
                } else if new_lmdb.map_size_bytes < cur {
                    inst.set_rebuilding(true);
                    let shrink_result = lmdb_backend.shrink_map_size(new_lmdb.map_size_bytes);
                    inst.set_rebuilding(false);
                    shrink_result.map_err(|e| {
                        tracing::error!(
                            cache = %name,
                            error = %e,
                            "LMDB map_size shrink failed; rejecting apply"
                        );
                        format!("cache '{name}': LMDB map_size shrink failed: {e}")
                    })?;
                    tracing::info!(
                        cache = %name,
                        from = cur,
                        to = new_lmdb.map_size_bytes,
                        "LMDB map_size shrunk"
                    );
                }
            }
            inst.config.store(Arc::new(cfg.clone()));
        }

        for (name, pending) in pending_opens {
            match pending {
                PendingBackendOpen::Replace {
                    backend,
                    cfg: opened_cfg,
                } => match guard.get(&name) {
                    Some(inst) => {
                        let old_cfg = inst.config.load_full();
                        let old_path = lmdb_path(old_cfg.as_ref())
                            .map(|p| p.display().to_string())
                            .unwrap_or_default();
                        let new_path = lmdb_path(&opened_cfg)
                            .map(|p| p.display().to_string())
                            .unwrap_or_default();
                        tracing::info!(
                            cache = %name,
                            from_type = old_cfg.backend_type.as_str(),
                            to_type = opened_cfg.backend_type.as_str(),
                            old_path = %old_path,
                            new_path = %new_path,
                            "cache backend replaced (type and/or LMDB path); entries not migrated"
                        );
                        inst.swap_backend(backend, opened_cfg);
                    }
                    None => {
                        guard.insert(
                            name,
                            Arc::new(CacheInstanceRuntime {
                                config: ArcSwap::from_pointee(opened_cfg),
                                backend: ArcSwap::from_pointee(backend),
                                rebuilding: AtomicBool::new(false),
                                inflight: InFlightTable::new(),
                                next_reap_shard: AtomicUsize::new(0),
                            }),
                        );
                    }
                },
                PendingBackendOpen::ShardReopen {
                    backend,
                    cfg: opened_cfg,
                    operator,
                    staging,
                } => {
                    let Some(inst) = guard.get(&name) else {
                        // Should not happen — shard reopen is only for existing instances.
                        let _ = std::fs::remove_dir_all(&staging);
                        continue;
                    };
                    let from_n = inst
                        .backend
                        .load()
                        .as_lmdb()
                        .map(|l| l.shard_count())
                        .unwrap_or(0);
                    let to_n = opened_cfg
                        .lmdb
                        .as_ref()
                        .and_then(|l| l.shard_count)
                        .unwrap_or(0);
                    tracing::info!(
                        cache = %name,
                        path = %operator.display(),
                        from_shards = from_n,
                        to_shards = to_n,
                        "LMDB shard_count layout replaced; entries not migrated"
                    );
                    // Swap first so the prior env is dropped before we delete its files.
                    inst.swap_backend(backend, opened_cfg);
                    if let Some(lmdb) = inst.backend.load().as_lmdb() {
                        if let Err(e) = lmdb.finalize_shard_reopen(&operator, &staging) {
                            // Arc swap already succeeded; the new envs remain open on renamed
                            // inodes. Log loudly — operator path may need manual repair.
                            tracing::error!(
                                cache = %name,
                                error = %e,
                                "LMDB shard_count reopen finalize failed after swap; \
                                 new layout is serving but path relocate did not complete"
                            );
                        }
                    }
                }
            }
        }

        let removed: Vec<String> = guard
            .keys()
            .filter(|name| !new.contains_key(name.as_str()))
            .cloned()
            .collect();
        for name in removed {
            tracing::info!(cache = %name, "dropping cache instance from registry");
            guard.remove(&name);
        }

        let _ = prev;
        Ok(())
    }

    pub fn set_async_coalesce(&self, enabled: bool) {
        self.async_coalesce.store(enabled, Ordering::Relaxed);
    }

    pub fn set_wake_handler(&self, wake: CacheWaitWake) {
        *self.wake.write() = Some(wake);
    }

    fn instance(&self, name: &str) -> Option<Arc<CacheInstanceRuntime>> {
        self.instances.read().get(name).cloned()
    }

    pub fn lookup(
        &self,
        cache_name: &str,
        txn: &crate::transaction::Transaction,
        now: Instant,
    ) -> CacheLookupOutcome {
        let Some(inst) = self.instance(cache_name) else {
            tracing::warn!(cache = cache_name, "unknown cache instance at runtime");
            return CacheLookupOutcome::Bypass;
        };

        if inst.is_rebuilding() {
            return CacheLookupOutcome::Bypass;
        }

        if !txn.cache_lookup_eligible {
            return CacheLookupOutcome::Bypass;
        }

        let key = match build_query_key(txn) {
            Ok(k) => k,
            Err(e) => {
                tracing::warn!(cache = cache_name, error = %e, "cache key build failed");
                return CacheLookupOutcome::Bypass;
            }
        };

        if let Some(entry) = self.try_read_hit(&inst, &key, txn, now) {
            return entry;
        }

        if let Some(entry) = try_ancestor_nxdomain_hit(&inst, txn, now) {
            return self.hit_from_entry(&inst, &entry, txn, now);
        }

        let cfg = inst.config.load();
        if cfg.truncated_udp.enabled && txn.protocol == crate::transaction::ClientProtocol::Udp {
            if let Ok(tc_key) = build_truncated_udp_key(txn) {
                if let Some(hit) = self.try_read_hit(&inst, &tc_key, txn, now) {
                    return hit;
                }
            }
        }

        let gate = inst.inflight.gate_for(&key);
        let async_coalesce = self.async_coalesce.load(Ordering::Relaxed);
        let role = gate.register(txn.id, async_coalesce);
        match role {
            InFlightRole::Leader => CacheLookupOutcome::Miss { key, gate },
            InFlightRole::Follower if async_coalesce => CacheLookupOutcome::WaitAsync { key },
            InFlightRole::Follower => {
                let _wire = gate.wait_for_result(Duration::from_secs(30));
                inst.inflight.remove(&key);
                let now = Instant::now();
                if let Some(entry) = inst.backend.load().get(&key, now) {
                    self.record_singleflight_coalesced(cache_name, txn);
                    return self.hit_from_entry(&inst, &entry, txn, now);
                }
                if let Some(w) = _wire {
                    self.record_singleflight_coalesced(cache_name, txn);
                    self.hit_from_stored(&inst, &w, txn, now)
                } else {
                    CacheLookupOutcome::Bypass
                }
            }
        }
    }

    fn try_read_hit(
        &self,
        inst: &CacheInstanceRuntime,
        key: &CacheKey,
        txn: &crate::transaction::Transaction,
        now: Instant,
    ) -> Option<CacheLookupOutcome> {
        let entry = inst.backend.load().get(key, now)?;
        Some(self.hit_from_entry(inst, &entry, txn, now))
    }

    fn hit_from_stored(
        &self,
        inst: &CacheInstanceRuntime,
        wire: &Arc<[u8]>,
        txn: &crate::transaction::Transaction,
        now: Instant,
    ) -> CacheLookupOutcome {
        let cfg = inst.config.load();
        // Coalesced follower when fill did not store (uncacheable wire): serve with zero age.
        let wire = match prepare_served_arc(
            wire,
            txn.dns_id,
            cfg.rotate_rrset_on_serve,
            now,
            now,
            Some(&txn.query_wire),
        ) {
            Ok(w) => w,
            Err(e) => {
                tracing::warn!(
                    cache = %cfg.name,
                    error = %e,
                    "failed to prepare coalesced wire"
                );
                return CacheLookupOutcome::Bypass;
            }
        };
        CacheLookupOutcome::Hit {
            wire,
            cache_name: cfg.name.clone(),
            skip_response_rules: matches!(
                cfg.on_hit_response_rules,
                conduit_config::lookup::OnHitResponseRules::Skip
            ),
        }
    }

    fn hit_from_entry(
        &self,
        inst: &CacheInstanceRuntime,
        entry: &CacheEntry,
        txn: &crate::transaction::Transaction,
        now: Instant,
    ) -> CacheLookupOutcome {
        let cfg = inst.config.load();
        let wire = match prepare_served_arc(
            &entry.wire,
            txn.dns_id,
            cfg.rotate_rrset_on_serve,
            entry.filled_at,
            now,
            Some(&txn.query_wire),
        ) {
            Ok(w) => w,
            Err(e) => {
                tracing::warn!(
                    cache = %cfg.name,
                    error = %e,
                    "failed to prepare cached wire"
                );
                return CacheLookupOutcome::Bypass;
            }
        };
        CacheLookupOutcome::Hit {
            wire,
            cache_name: cfg.name.clone(),
            skip_response_rules: matches!(
                cfg.on_hit_response_rules,
                conduit_config::lookup::OnHitResponseRules::Skip
            ),
        }
    }

    pub fn fill_from_forward(
        &self,
        cache_name: &str,
        key: &CacheKey,
        gate: &Arc<super::inflight::InFlightGate>,
        wire: Arc<[u8]>,
        txn: &crate::transaction::Transaction,
    ) {
        let Some(inst) = self.instance(cache_name) else {
            tracing::warn!(
                cache = cache_name,
                "cache fill skipped: unknown cache instance at runtime"
            );
            return;
        };
        let cfg = inst.config.load_full();
        let backend = inst.backend.load();
        let now = Instant::now();

        if inst.is_rebuilding() {
            tracing::debug!(
                cache = cache_name,
                "cache fill skipped: instance rebuilding (map_size shrink)"
            );
            let _ = gate.complete(None);
            inst.inflight.remove(key);
            return;
        }

        let store_key = if should_store_truncated(cfg.as_ref(), &wire, txn) {
            build_truncated_udp_key(txn)
                .map(|k| (k, cfg.truncated_udp.ttl_secs))
                .ok()
        } else {
            None
        };

        if store_key.is_none() && is_truncated_udp_wire(&wire, txn) {
            tracing::debug!(
                cache = cache_name,
                "truncated UDP response not stored (policy disabled or ineligible wire)"
            );
            let _ = gate.complete(None);
            inst.inflight.remove(key);
            return;
        }

        let storing_truncated = store_key.is_some();
        let (insert_key, override_ttl) = if let Some((k, ttl)) = store_key {
            (k, Some(ttl))
        } else {
            (key.clone(), None)
        };

        let mut entry = match entry_from_wire(
            wire.clone(),
            cfg.negative_cache.enabled,
            cfg.negative_cache.servfail_ttl_secs,
            now,
        ) {
            Some(e) => e,
            None => {
                tracing::debug!(
                    cache = cache_name,
                    "wire not cacheable; completing in-flight without store"
                );
                let _ = gate.complete(None);
                inst.inflight.remove(key);
                return;
            }
        };

        if let Some(ttl) = override_ttl {
            entry.expires_at = super::entry::expires_at_from_ttl(now, ttl);
        }

        // Insert the complete answer first so lookups (which prefer the complete
        // key) never see a gap. Then drop any truncated-UDP sibling.
        let fill_started = Instant::now();
        let outcome = backend.insert(insert_key, entry, now);
        let fill_secs = fill_started.elapsed().as_secs_f64();
        if outcome.evictions > 0 {
            self.record_cache_evictions(cache_name, "when_full", outcome.evictions);
            self.observe_cache_eviction_duration(cache_name, "when_full", outcome.eviction_secs);
        }
        if outcome.stored && !storing_truncated {
            if let Ok(tc_key) = build_truncated_udp_key(txn) {
                if backend.remove(&tc_key) {
                    tracing::debug!(
                        cache = cache_name,
                        "removed truncated UDP sibling after complete cache fill"
                    );
                }
            }
        }
        if outcome.stored {
            self.record_cache_fill(cache_name, txn);
            self.observe_cache_fill_duration(cache_name, txn, fill_secs);
        } else if backend.as_lmdb().is_some() {
            self.record_cache_lmdb_error(cache_name, "capacity_pressure");
        }
        let waiters = gate.complete(Some(wire));
        inst.inflight.remove(key);
        self.wake_waiters(waiters);
    }

    pub fn instance_gate(
        &self,
        cache_name: &str,
        key: &CacheKey,
    ) -> Option<Arc<super::inflight::InFlightGate>> {
        let inst = self.instance(cache_name)?;
        Some(inst.inflight.gate_for(key))
    }

    pub fn complete_inflight_miss(
        &self,
        cache_name: &str,
        key: &CacheKey,
        gate: &Arc<super::inflight::InFlightGate>,
    ) {
        let waiters = gate.complete(None);
        if let Some(inst) = self.instance(cache_name) {
            inst.inflight.remove(key);
        }
        self.wake_waiters(waiters);
    }

    pub fn resume_after_wait(
        &self,
        cache_name: &str,
        key: &CacheKey,
        txn: &crate::transaction::Transaction,
    ) -> CacheLookupOutcome {
        let Some(inst) = self.instance(cache_name) else {
            return CacheLookupOutcome::Bypass;
        };
        let gate = inst.inflight.gate_for(key);
        let now = Instant::now();
        if let Some(entry) = inst.backend.load().get(key, now) {
            inst.inflight.remove(key);
            self.record_singleflight_coalesced(cache_name, txn);
            return self.hit_from_entry(&inst, &entry, txn, now);
        }
        if let Some(wire) = gate.result() {
            inst.inflight.remove(key);
            self.record_singleflight_coalesced(cache_name, txn);
            return self.hit_from_stored(&inst, &wire, txn, now);
        }
        CacheLookupOutcome::Bypass
    }

    fn wake_waiters(&self, waiters: Vec<u64>) {
        let wake = self.wake.read().clone();
        if let Some(w) = wake {
            for id in waiters {
                w(id);
            }
        }
    }

    pub fn entry_count(&self, cache_name: &str) -> u64 {
        self.instance(cache_name)
            .map(|i| i.backend.load().entry_count())
            .unwrap_or(0)
    }

    pub fn max_entries(&self, cache_name: &str) -> Option<u64> {
        self.instance(cache_name).map(|i| i.max_entries())
    }

    pub fn all_entry_counts(&self) -> Vec<(String, u64)> {
        self.instances
            .read()
            .iter()
            .map(|(name, inst)| (name.clone(), inst.backend.load().entry_count()))
            .collect()
    }

    /// Capacity samples for scrape-time gauges (entries/bytes limits and usage).
    pub fn all_capacity_samples(&self) -> Vec<conduit_metrics::CacheCapacitySample> {
        self.instances
            .read()
            .iter()
            .map(|(name, inst)| {
                let be = inst.backend.load();
                let entries = be.entry_count();
                let entries_limit = be.max_entries();
                let (bytes_used, bytes_limit, lmdb_shards) = match be.as_lmdb() {
                    Some(lmdb) => (
                        lmdb.used_bytes(),
                        lmdb.map_size_bytes(),
                        lmdb.shard_count() as u64,
                    ),
                    None => (0, 0, 0),
                };
                conduit_metrics::CacheCapacitySample {
                    cache: name.clone(),
                    entries,
                    entries_limit,
                    bytes_used,
                    bytes_limit,
                    lmdb_shards,
                }
            })
            .collect()
    }

    /// Whether any instance is configured for active (background) eviction.
    pub fn has_active_eviction(&self) -> bool {
        self.instances.read().values().any(|inst| {
            matches!(
                inst.config.load().memory.eviction,
                conduit_config::lookup::EvictionMode::Active
            )
        })
    }

    /// Reap expired entries for instances with `memory.eviction: active`.
    ///
    /// Uses [`ReapBudget::DEFAULT`] (fixed until operator knobs land). Returns the
    /// total number of entries removed. Records
    /// `conduit_cache_evictions_total{reason="active_reaper"}` when metrics are attached.
    pub fn reap_active_expired(&self, now: Instant) -> u64 {
        self.reap_active_expired_with_budget(now, ReapBudget::DEFAULT)
    }

    pub fn reap_active_expired_with_budget(&self, now: Instant, budget: ReapBudget) -> u64 {
        let instances: Vec<(String, Arc<CacheInstanceRuntime>)> = self
            .instances
            .read()
            .iter()
            .filter(|(_, inst)| {
                matches!(
                    inst.config.load().memory.eviction,
                    conduit_config::lookup::EvictionMode::Active
                )
            })
            .map(|(name, inst)| (name.clone(), Arc::clone(inst)))
            .collect();

        let mut total = 0u64;
        for (name, inst) in instances {
            let start = inst.next_reap_shard.load(Ordering::Relaxed);
            let mut cursor = ReapCursor { next_shard: start };
            let outcome = inst
                .backend
                .load()
                .reap_expired_budgeted(now, budget, &mut cursor);
            inst.next_reap_shard
                .store(cursor.next_shard, Ordering::Relaxed);
            if outcome.removed > 0 {
                self.record_cache_evictions(&name, "active_reaper", outcome.removed);
                total += outcome.removed;
            }
        }
        total
    }

    fn record_cache_fill(&self, cache_name: &str, txn: &crate::transaction::Transaction) {
        let hub = self.metrics.read();
        let Some(hub) = hub.as_ref() else {
            return;
        };
        if !hub.metrics_enabled() {
            return;
        }
        let profile = txn
            .lookup_profile
            .as_deref()
            .unwrap_or(conduit_config::lookup::DEFAULT_LOOKUP_PROFILE);
        hub.builtin().record_cache_fill(cache_name, profile);
    }

    fn observe_cache_fill_duration(
        &self,
        cache_name: &str,
        txn: &crate::transaction::Transaction,
        duration_secs: f64,
    ) {
        let hub = self.metrics.read();
        let Some(hub) = hub.as_ref() else {
            return;
        };
        if !hub.metrics_enabled() {
            return;
        }
        let profile = txn
            .lookup_profile
            .as_deref()
            .unwrap_or(conduit_config::lookup::DEFAULT_LOOKUP_PROFILE);
        hub.builtin()
            .observe_cache_fill_duration(cache_name, profile, duration_secs);
    }

    fn observe_cache_eviction_duration(&self, cache_name: &str, reason: &str, duration_secs: f64) {
        if duration_secs <= 0.0 {
            return;
        }
        let hub = self.metrics.read();
        let Some(hub) = hub.as_ref() else {
            return;
        };
        if !hub.metrics_enabled() {
            return;
        }
        hub.builtin()
            .observe_cache_eviction_duration(cache_name, reason, duration_secs);
    }

    fn record_singleflight_coalesced(
        &self,
        cache_name: &str,
        txn: &crate::transaction::Transaction,
    ) {
        let hub = self.metrics.read();
        let Some(hub) = hub.as_ref() else {
            return;
        };
        if !hub.metrics_enabled() {
            return;
        }
        let profile = txn
            .lookup_profile
            .as_deref()
            .unwrap_or(conduit_config::lookup::DEFAULT_LOOKUP_PROFILE);
        hub.builtin()
            .record_cache_singleflight_coalesced(cache_name, profile);
    }

    fn record_cache_evictions(&self, cache_name: &str, reason: &str, count: u64) {
        if count == 0 {
            return;
        }
        let hub = self.metrics.read();
        let Some(hub) = hub.as_ref() else {
            return;
        };
        if !hub.metrics_enabled() {
            return;
        }
        hub.builtin()
            .record_cache_evictions(cache_name, reason, count);
    }

    fn record_cache_lmdb_error(&self, cache_name: &str, reason: &str) {
        let hub = self.metrics.read();
        let Some(hub) = hub.as_ref() else {
            return;
        };
        if !hub.metrics_enabled() {
            return;
        }
        hub.builtin().record_cache_lmdb_error(cache_name, reason);
    }
}

fn should_store_truncated(
    cfg: &CompiledCacheInstance,
    wire: &[u8],
    txn: &crate::transaction::Transaction,
) -> bool {
    if !cfg.truncated_udp.enabled {
        return false;
    }
    is_truncated_udp_wire(wire, txn)
}

fn is_truncated_udp_wire(wire: &[u8], txn: &crate::transaction::Transaction) -> bool {
    if txn.protocol != crate::transaction::ClientProtocol::Udp {
        return false;
    }
    hickory_proto::op::Message::from_vec(wire)
        .ok()
        .map(|m| m.header().truncated())
        .unwrap_or(false)
}

fn lmdb_path(cfg: &CompiledCacheInstance) -> Option<&std::path::Path> {
    cfg.lmdb.as_ref().map(|l| l.path.as_path())
}

enum PendingBackendOpen {
    Replace {
        backend: CacheBackend,
        cfg: CompiledCacheInstance,
    },
    /// Explicit `shard_count` differs from the live on-disk layout — staging open + finalize.
    ShardReopen {
        backend: CacheBackend,
        cfg: CompiledCacheInstance,
        operator: std::path::PathBuf,
        staging: std::path::PathBuf,
    },
}

/// Warm reopen when new config has an explicit shard_count that differs from the live layout.
fn lmdb_explicit_shard_reopen_needed(new: &CompiledCacheInstance, live: &CacheBackend) -> bool {
    let Some(new_lmdb) = new.lmdb.as_ref() else {
        return false;
    };
    let Some(explicit) = new_lmdb.shard_count else {
        return false;
    };
    let Some(live_lmdb) = live.as_lmdb() else {
        return false;
    };
    let want = explicit.clamp(1, conduit_config::lookup::MAX_LMDB_SHARD_COUNT) as usize;
    if live_lmdb.layout_is_legacy() {
        return want != 1;
    }
    want != live_lmdb.shard_count()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transaction::{ClientProtocol, Transaction};
    use conduit_config::lookup::{
        CacheBackendType, CompiledCacheInstance, CompiledMemoryCache, CompiledNegativeCache,
        CompiledTruncatedUdp, EvictionMode, OnHitResponseRules,
    };
    use hickory_proto::op::{Message, MessageType, Query, ResponseCode};
    use hickory_proto::rr::{Name, RecordType};
    use hickory_proto::serialize::binary::{BinEncodable, BinEncoder};
    use std::collections::HashMap;
    use std::net::SocketAddr;

    fn test_cache_instance(covers_descendants: bool) -> CompiledCacheInstance {
        test_cache_instance_with_truncated(covers_descendants, false)
    }

    fn test_cache_instance_with_truncated(
        covers_descendants: bool,
        truncated_udp_enabled: bool,
    ) -> CompiledCacheInstance {
        CompiledCacheInstance {
            name: "global".into(),
            backend_type: CacheBackendType::Memory,
            negative_cache: CompiledNegativeCache {
                enabled: true,
                nxdomain_covers_descendants: covers_descendants,
                servfail_ttl_secs: 10,
            },
            on_hit_response_rules: OnHitResponseRules::Run,
            truncated_udp: CompiledTruncatedUdp {
                enabled: truncated_udp_enabled,
                ttl_secs: 60,
            },
            rotate_rrset_on_serve: false,
            memory: CompiledMemoryCache {
                shard_count: 4,
                eviction: EvictionMode::Passive,
            },
            lmdb: None,
            max_entries: 1000,
        }
    }

    fn nxdomain_wire(qname: &str) -> Arc<[u8]> {
        // from_ascii preserves label case (needed for 0x20 echo tests).
        let name = Name::from_ascii(qname).unwrap();
        let mut msg = Message::new();
        msg.set_message_type(MessageType::Response);
        msg.set_response_code(ResponseCode::NXDomain);
        msg.add_query(Query::query(name, RecordType::A));
        let mut buf = Vec::new();
        let mut enc = BinEncoder::new(&mut buf);
        msg.emit(&mut enc).unwrap();
        buf.into()
    }

    fn client_query(qname: &str) -> Vec<u8> {
        // from_ascii preserves label case so mixed-case 0x20 queries survive encode/decode.
        let name = Name::from_ascii(qname).unwrap();
        let mut msg = Message::new();
        msg.add_query(Query::query(name, RecordType::A));
        let mut buf = Vec::new();
        let mut enc = BinEncoder::new(&mut buf);
        msg.emit(&mut enc).unwrap();
        buf
    }

    fn positive_a_wire(qname: &str) -> Arc<[u8]> {
        use hickory_proto::rr::{RData, Record};
        let name = Name::from_ascii(qname).unwrap();
        let mut msg = Message::new();
        msg.set_message_type(MessageType::Response);
        msg.set_response_code(ResponseCode::NoError);
        msg.add_query(Query::query(name.clone(), RecordType::A));
        msg.add_answer(Record::from_rdata(
            name,
            300,
            RData::A(hickory_proto::rr::rdata::A(std::net::Ipv4Addr::new(
                192, 0, 2, 10,
            ))),
        ));
        let mut buf = Vec::new();
        let mut enc = BinEncoder::new(&mut buf);
        msg.emit(&mut enc).unwrap();
        buf.into()
    }

    /// Positive answer with authority NS and glue in additional (sections must survive fill+serve).
    fn multi_section_positive_wire(qname: &str) -> Arc<[u8]> {
        use hickory_proto::rr::rdata::{A, NS};
        use hickory_proto::rr::{RData, Record};
        let name = Name::from_ascii(qname).unwrap();
        let zone = Name::from_ascii("example.").unwrap();
        let ns = Name::from_ascii("ns.example.").unwrap();
        let mut msg = Message::new();
        msg.set_message_type(MessageType::Response);
        msg.set_response_code(ResponseCode::NoError);
        msg.add_query(Query::query(name.clone(), RecordType::A));
        msg.add_answer(Record::from_rdata(
            name,
            300,
            RData::A(A::new(192, 0, 2, 10)),
        ));
        msg.add_name_server(Record::from_rdata(zone, 600, RData::NS(NS(ns.clone()))));
        msg.add_additional(Record::from_rdata(ns, 600, RData::A(A::new(192, 0, 2, 53))));
        let mut buf = Vec::new();
        let mut enc = BinEncoder::new(&mut buf);
        msg.emit(&mut enc).unwrap();
        buf.into()
    }

    #[test]
    fn fill_preserves_authority_and_additional_sections() {
        let mut instances = HashMap::new();
        instances.insert("global".into(), test_cache_instance(true));
        let registry = LookupCacheRegistry::from_snapshot(&instances);

        let qname = "sections.policy-lab.test.example.";
        let txn = txn_for(qname, 40);
        let key = build_query_key(&txn).unwrap();
        let gate = registry.instance_gate("global", &key).unwrap();
        let filled = multi_section_positive_wire(qname);
        let filled_msg = Message::from_vec(&filled).unwrap();
        assert_eq!(filled_msg.name_servers().len(), 1);
        assert_eq!(filled_msg.additionals().len(), 1);

        registry.fill_from_forward("global", &key, &gate, filled.clone(), &txn);

        let hit_txn = txn_for(qname, 41);
        match registry.lookup("global", &hit_txn, Instant::now()) {
            CacheLookupOutcome::Hit { wire, .. } => {
                let served = Message::from_vec(&wire).unwrap();
                assert_eq!(served.answers().len(), 1, "answer section present");
                assert_eq!(
                    served.name_servers().len(),
                    1,
                    "authority NS must survive fill+serve"
                );
                assert_eq!(
                    served.additionals().len(),
                    1,
                    "additional glue must survive fill+serve"
                );
                assert_eq!(
                    served.name_servers()[0].data(),
                    filled_msg.name_servers()[0].data()
                );
                assert_eq!(
                    served.additionals()[0].data(),
                    filled_msg.additionals()[0].data()
                );
            }
            _ => panic!("expected Hit, got non-Hit outcome"),
        }

        // Stored slab bytes must still include all sections (serve path clones, does not mutate).
        let inst = registry.instance("global").unwrap();
        let entry = inst
            .backend
            .load()
            .get(&key, Instant::now())
            .expect("stored entry");
        let stored = Message::from_vec(&entry.wire).unwrap();
        assert_eq!(stored.name_servers().len(), 1);
        assert_eq!(stored.additionals().len(), 1);
    }

    fn truncated_positive_wire(qname: &str) -> Arc<[u8]> {
        use hickory_proto::rr::{RData, Record};
        let name = Name::from_ascii(qname).unwrap();
        let mut msg = Message::new();
        msg.set_message_type(MessageType::Response);
        msg.set_truncated(true);
        msg.add_query(Query::query(name.clone(), RecordType::A));
        msg.add_answer(Record::from_rdata(
            name,
            120,
            RData::A(hickory_proto::rr::rdata::A(std::net::Ipv4Addr::new(
                192, 0, 2, 1,
            ))),
        ));
        let mut buf = Vec::new();
        let mut enc = BinEncoder::new(&mut buf);
        msg.emit(&mut enc).unwrap();
        buf.into()
    }

    #[test]
    fn truncated_udp_stored_and_hit_when_policy_enabled() {
        let mut instances = HashMap::new();
        instances.insert(
            "global".into(),
            test_cache_instance_with_truncated(true, true),
        );
        let registry = LookupCacheRegistry::from_snapshot(&instances);
        let txn = txn_for("tc.policy-lab.test.example.", 10);
        let key = build_query_key(&txn).unwrap();
        let gate = registry.instance_gate("global", &key).unwrap();
        registry.fill_from_forward(
            "global",
            &key,
            &gate,
            truncated_positive_wire("tc.policy-lab.test.example."),
            &txn,
        );

        let now = Instant::now();
        assert!(matches!(
            registry.lookup("global", &txn, now),
            CacheLookupOutcome::Hit { .. }
        ));
    }

    #[test]
    fn truncated_udp_wire_not_stored_when_policy_disabled() {
        let mut instances = HashMap::new();
        instances.insert("global".into(), test_cache_instance(true));
        let registry = LookupCacheRegistry::from_snapshot(&instances);
        let txn = txn_for("tc.policy-lab.test.example.", 10);
        let key = build_query_key(&txn).unwrap();
        let gate = registry.instance_gate("global", &key).unwrap();
        registry.fill_from_forward(
            "global",
            &key,
            &gate,
            truncated_positive_wire("tc.policy-lab.test.example."),
            &txn,
        );

        let now = Instant::now();
        assert!(matches!(
            registry.lookup("global", &txn, now),
            CacheLookupOutcome::Miss { .. }
        ));
    }

    #[test]
    fn complete_answer_filled_over_udp_hits_for_tcp_client() {
        let mut instances = HashMap::new();
        instances.insert("global".into(), test_cache_instance(true));
        let registry = LookupCacheRegistry::from_snapshot(&instances);

        let udp_txn = txn_for("shared.policy-lab.test.example.", 20);
        let key = build_query_key(&udp_txn).unwrap();
        let gate = registry.instance_gate("global", &key).unwrap();
        registry.fill_from_forward(
            "global",
            &key,
            &gate,
            nxdomain_wire("shared.policy-lab.test.example."),
            &udp_txn,
        );

        let tcp_txn = txn_for_protocol("shared.policy-lab.test.example.", 21, ClientProtocol::Tcp);
        assert!(matches!(
            registry.lookup("global", &tcp_txn, Instant::now()),
            CacheLookupOutcome::Hit { .. }
        ));
    }

    #[test]
    fn truncated_udp_entry_not_served_to_tcp_client() {
        let mut instances = HashMap::new();
        instances.insert(
            "global".into(),
            test_cache_instance_with_truncated(true, true),
        );
        let registry = LookupCacheRegistry::from_snapshot(&instances);
        let udp_txn = txn_for("tc.tcp-skip.policy-lab.test.example.", 30);
        let key = build_query_key(&udp_txn).unwrap();
        let gate = registry.instance_gate("global", &key).unwrap();
        registry.fill_from_forward(
            "global",
            &key,
            &gate,
            truncated_positive_wire("tc.tcp-skip.policy-lab.test.example."),
            &udp_txn,
        );

        assert!(matches!(
            registry.lookup("global", &udp_txn, Instant::now()),
            CacheLookupOutcome::Hit { .. }
        ));

        let tcp_txn = txn_for_protocol(
            "tc.tcp-skip.policy-lab.test.example.",
            31,
            ClientProtocol::Tcp,
        );
        assert!(matches!(
            registry.lookup("global", &tcp_txn, Instant::now()),
            CacheLookupOutcome::Miss { .. }
        ));
    }

    #[test]
    fn complete_fill_removes_truncated_udp_sibling() {
        let mut instances = HashMap::new();
        instances.insert(
            "global".into(),
            test_cache_instance_with_truncated(true, true),
        );
        let registry = LookupCacheRegistry::from_snapshot(&instances);
        let qname = "replace-tc.policy-lab.test.example.";

        let udp_txn = txn_for(qname, 40);
        let complete_key = build_query_key(&udp_txn).unwrap();
        let tc_key = build_truncated_udp_key(&udp_txn).unwrap();
        let gate = registry.instance_gate("global", &complete_key).unwrap();
        registry.fill_from_forward(
            "global",
            &complete_key,
            &gate,
            truncated_positive_wire(qname),
            &udp_txn,
        );
        assert_eq!(registry.entry_count("global"), 1);
        match registry.lookup("global", &udp_txn, Instant::now()) {
            CacheLookupOutcome::Hit { wire, .. } => {
                let msg = Message::from_vec(&wire).unwrap();
                assert!(msg.truncated(), "precondition: UDP hit is the TC stub");
            }
            _other => panic!("expected truncated UDP hit before complete fill"),
        }

        let tcp_txn = txn_for_protocol(qname, 41, ClientProtocol::Tcp);
        let tcp_gate = registry.instance_gate("global", &complete_key).unwrap();
        registry.fill_from_forward(
            "global",
            &complete_key,
            &tcp_gate,
            positive_a_wire(qname),
            &tcp_txn,
        );

        assert_eq!(
            registry.entry_count("global"),
            1,
            "complete fill must replace the truncated sibling, not leave two entries"
        );
        // Truncated key must be gone (lookup via backend through a fresh get path).
        let inst = registry.instance("global").expect("cache instance");
        assert!(
            inst.backend.load().get(&tc_key, Instant::now()).is_none(),
            "truncated UDP sibling key must be removed"
        );

        match registry.lookup("global", &udp_txn, Instant::now()) {
            CacheLookupOutcome::Hit { wire, .. } => {
                let msg = Message::from_vec(&wire).unwrap();
                assert!(
                    !msg.truncated(),
                    "UDP should now be served the complete cached answer"
                );
                assert_eq!(msg.answers().len(), 1);
            }
            _other => panic!("expected complete cache hit for UDP after TCP fill"),
        }
    }

    fn txn_for(qname: &str, id: u64) -> Transaction {
        txn_for_protocol(qname, id, ClientProtocol::Udp)
    }

    fn txn_for_protocol(qname: &str, id: u64, protocol: ClientProtocol) -> Transaction {
        let addr: SocketAddr = "127.0.0.1:53".parse().unwrap();
        let wire = client_query(qname);
        let mut txn = Transaction::new(id, addr, protocol);
        txn.qname = Some(qname.into());
        txn.qtype = Some(1);
        txn.qclass = Some(1);
        txn.query_wire = wire;
        txn.dns_id = 0x1234;
        txn
    }

    #[test]
    fn exact_hit_echoes_client_question_case_0x20() {
        let mut instances = HashMap::new();
        instances.insert("global".into(), test_cache_instance(true));
        let registry = LookupCacheRegistry::from_snapshot(&instances);

        // Fill with a lowercase question in the stored wire.
        let fill_txn = txn_for("www.0x20-echo.example.", 100);
        let key = build_query_key(&fill_txn).unwrap();
        let gate = registry.instance_gate("global", &key).unwrap();
        registry.fill_from_forward(
            "global",
            &key,
            &gate,
            positive_a_wire("www.0x20-echo.example."),
            &fill_txn,
        );

        // Later client uses mixed-case QNAME (0x20 encoding); cache key matches case-insensitively.
        let mut hit_txn = txn_for("WwW.0X20-eChO.eXaMpLe.", 101);
        hit_txn.dns_id = 0xbeef;
        match registry.lookup("global", &hit_txn, Instant::now()) {
            CacheLookupOutcome::Hit { wire, .. } => {
                let msg = Message::from_vec(&wire).unwrap();
                assert_eq!(msg.id(), 0xbeef);
                assert_eq!(
                    msg.queries()[0].name().to_utf8(),
                    "WwW.0X20-eChO.eXaMpLe.",
                    "cache hit must echo the client's 0x20 question encoding"
                );
                assert_eq!(msg.response_code(), ResponseCode::NoError);
                assert_eq!(msg.answers().len(), 1);
            }
            _other => panic!("expected exact cache hit, got unexpected outcome"),
        }
    }

    #[test]
    fn truncated_udp_hit_echoes_client_question_case_0x20() {
        let mut instances = HashMap::new();
        instances.insert(
            "global".into(),
            test_cache_instance_with_truncated(true, true),
        );
        let registry = LookupCacheRegistry::from_snapshot(&instances);

        let fill_txn = txn_for("tc.0x20-echo.example.", 110);
        let key = build_query_key(&fill_txn).unwrap();
        let gate = registry.instance_gate("global", &key).unwrap();
        registry.fill_from_forward(
            "global",
            &key,
            &gate,
            truncated_positive_wire("tc.0x20-echo.example."),
            &fill_txn,
        );

        let hit_txn = txn_for("Tc.0X20-eChO.eXaMpLe.", 111);
        match registry.lookup("global", &hit_txn, Instant::now()) {
            CacheLookupOutcome::Hit { wire, .. } => {
                let msg = Message::from_vec(&wire).unwrap();
                assert!(msg.truncated());
                assert_eq!(
                    msg.queries()[0].name().to_utf8(),
                    "Tc.0X20-eChO.eXaMpLe.",
                    "truncated UDP cache hit must echo the client's 0x20 question encoding"
                );
            }
            _other => panic!("expected truncated UDP cache hit, got unexpected outcome"),
        }
    }

    #[test]
    fn ancestor_nxdomain_echoes_descendant_question_case_0x20() {
        let mut instances = HashMap::new();
        instances.insert("global".into(), test_cache_instance(true));
        let registry = LookupCacheRegistry::from_snapshot(&instances);
        let now = Instant::now();

        let parent_txn = txn_for("8020.0x20-echo.example.", 120);
        let parent_key = build_query_key(&parent_txn).unwrap();
        let gate = registry.instance_gate("global", &parent_key).unwrap();
        registry.fill_from_forward(
            "global",
            &parent_key,
            &gate,
            nxdomain_wire("8020.0x20-echo.example."),
            &parent_txn,
        );

        let child_txn = txn_for("ChIlD.8020.0x20-eChO.eXaMpLe.", 121);
        match registry.lookup("global", &child_txn, now) {
            CacheLookupOutcome::Hit { wire, .. } => {
                let msg = Message::from_vec(&wire).unwrap();
                assert_eq!(msg.response_code(), ResponseCode::NXDomain);
                assert_eq!(
                    msg.queries()[0].name().to_utf8(),
                    "ChIlD.8020.0x20-eChO.eXaMpLe.",
                    "ancestor NXDOMAIN hit must echo the descendant's 0x20 question encoding"
                );
            }
            _other => panic!("expected ancestor cache hit, got unexpected outcome"),
        }
    }

    #[test]
    fn ancestor_nxdomain_covers_descendant_query() {
        let mut instances = HashMap::new();
        instances.insert("global".into(), test_cache_instance(true));
        let registry = LookupCacheRegistry::from_snapshot(&instances);
        let now = Instant::now();

        let parent_txn = txn_for("8020.policy-lab.test.example.", 1);
        let parent_key = build_query_key(&parent_txn).unwrap();
        let gate = registry.instance_gate("global", &parent_key).unwrap();
        registry.fill_from_forward(
            "global",
            &parent_key,
            &gate,
            nxdomain_wire("8020.policy-lab.test.example."),
            &parent_txn,
        );

        let child_txn = txn_for("child.8020.policy-lab.test.example.", 2);
        match registry.lookup("global", &child_txn, now) {
            CacheLookupOutcome::Hit { wire, .. } => {
                let msg = Message::from_vec(&wire).unwrap();
                assert_eq!(msg.response_code(), ResponseCode::NXDomain);
                assert_eq!(
                    msg.queries()[0].name().to_utf8(),
                    "child.8020.policy-lab.test.example."
                );
            }
            _other => panic!("expected ancestor cache hit, got unexpected outcome"),
        }
    }

    #[test]
    fn ancestor_nxdomain_skipped_when_knob_off() {
        let mut instances = HashMap::new();
        instances.insert("global".into(), test_cache_instance(false));
        let registry = LookupCacheRegistry::from_snapshot(&instances);
        let now = Instant::now();

        let parent_txn = txn_for("8020.policy-lab.test.example.", 3);
        let parent_key = build_query_key(&parent_txn).unwrap();
        let gate = registry.instance_gate("global", &parent_key).unwrap();
        registry.fill_from_forward(
            "global",
            &parent_key,
            &gate,
            nxdomain_wire("8020.policy-lab.test.example."),
            &parent_txn,
        );

        let child_txn = txn_for("child.8020.policy-lab.test.example.", 4);
        match registry.lookup("global", &child_txn, now) {
            CacheLookupOutcome::Miss { .. } => {}
            _other => panic!("expected cache miss when knob off, got unexpected outcome"),
        }
    }

    #[test]
    fn reap_active_expired_only_touches_active_instances() {
        let mut active_cfg = test_cache_instance(true);
        active_cfg.name = "active".into();
        active_cfg.memory.eviction = EvictionMode::Active;
        let mut passive_cfg = test_cache_instance(true);
        passive_cfg.name = "passive".into();
        passive_cfg.memory.eviction = EvictionMode::Passive;

        let mut instances = HashMap::new();
        instances.insert("active".into(), active_cfg);
        instances.insert("passive".into(), passive_cfg);
        let registry = LookupCacheRegistry::from_snapshot(&instances);

        let now = Instant::now();
        let stale = CacheEntry {
            kind: EntryKind::Positive,
            wire: nxdomain_wire("stale.example."),
            filled_at: now - std::time::Duration::from_secs(60),
            expires_at: now - std::time::Duration::from_secs(1),
        };
        let fresh = CacheEntry {
            kind: EntryKind::Positive,
            wire: nxdomain_wire("fresh.example."),
            filled_at: now,
            expires_at: now + std::time::Duration::from_secs(120),
        };

        let active = registry.instance("active").unwrap();
        active
            .backend
            .load()
            .insert(CacheKey(b"a-stale".to_vec()), stale.clone(), now);
        active
            .backend
            .load()
            .insert(CacheKey(b"a-fresh".to_vec()), fresh.clone(), now);
        let passive = registry.instance("passive").unwrap();
        passive
            .backend
            .load()
            .insert(CacheKey(b"p-stale".to_vec()), stale, now);
        passive
            .backend
            .load()
            .insert(CacheKey(b"p-fresh".to_vec()), fresh, now);

        assert_eq!(registry.entry_count("active"), 2);
        assert_eq!(registry.entry_count("passive"), 2);

        let removed = registry.reap_active_expired(now);
        assert_eq!(
            removed, 1,
            "only the active instance's stale entry is reaped"
        );
        assert_eq!(registry.entry_count("active"), 1);
        assert_eq!(
            registry.entry_count("passive"),
            2,
            "passive must leave expired entries until insert pressure"
        );
    }

    #[test]
    fn has_active_eviction_reflects_instance_modes() {
        let mut passive_only = HashMap::new();
        passive_only.insert("p".into(), test_cache_instance(true));
        let passive_reg = LookupCacheRegistry::from_snapshot(&passive_only);
        assert!(!passive_reg.has_active_eviction());

        let mut with_active = HashMap::new();
        let mut cfg = test_cache_instance(true);
        cfg.memory.eviction = EvictionMode::Active;
        with_active.insert("a".into(), cfg);
        let active_reg = LookupCacheRegistry::from_snapshot(&with_active);
        assert!(active_reg.has_active_eviction());
    }

    #[test]
    fn reconcile_lowers_max_entries_and_trims_existing_backend() {
        let mut prev_map = HashMap::new();
        let mut cfg = test_cache_instance(true);
        cfg.max_entries = 0;
        prev_map.insert("global".into(), cfg);
        let registry = LookupCacheRegistry::from_snapshot(&prev_map);

        for i in 0..5 {
            let qname = format!("reconcile-max-{i}.example.");
            let txn = txn_for(&qname, i + 1);
            let key = build_query_key(&txn).unwrap();
            let gate = registry.instance_gate("global", &key).unwrap();
            registry.fill_from_forward("global", &key, &gate, nxdomain_wire(&qname), &txn);
        }
        assert_eq!(registry.entry_count("global"), 5);

        let mut new_map = prev_map.clone();
        new_map.get_mut("global").unwrap().max_entries = 2;
        registry.reconcile(&prev_map, &new_map).unwrap();

        assert_eq!(registry.max_entries("global"), Some(2));
        assert_eq!(registry.entry_count("global"), 2);
    }

    #[test]
    fn reconcile_adds_new_instance_without_dropping_existing() {
        let mut prev_map = HashMap::new();
        prev_map.insert("global".into(), test_cache_instance(true));
        let registry = LookupCacheRegistry::from_snapshot(&prev_map);

        let txn = txn_for("new-instance.example.", 1);
        let key = build_query_key(&txn).unwrap();
        let gate = registry.instance_gate("global", &key).unwrap();
        registry.fill_from_forward(
            "global",
            &key,
            &gate,
            nxdomain_wire("new-instance.example."),
            &txn,
        );
        assert_eq!(registry.entry_count("global"), 1);

        let mut new_map = prev_map.clone();
        let mut other = test_cache_instance(true);
        other.name = "regional".into();
        new_map.insert("regional".into(), other);
        registry.reconcile(&prev_map, &new_map).unwrap();

        assert_eq!(registry.entry_count("global"), 1);
        assert_eq!(registry.entry_count("regional"), 0);
    }

    fn lmdb_cache_instance(path: std::path::PathBuf, map_size_bytes: u64) -> CompiledCacheInstance {
        lmdb_cache_instance_shards(path, map_size_bytes, Some(1))
    }

    fn lmdb_cache_instance_shards(
        path: std::path::PathBuf,
        map_size_bytes: u64,
        shard_count: Option<u32>,
    ) -> CompiledCacheInstance {
        CompiledCacheInstance {
            name: "durable".into(),
            backend_type: CacheBackendType::Lmdb,
            negative_cache: CompiledNegativeCache {
                enabled: true,
                nxdomain_covers_descendants: true,
                servfail_ttl_secs: 10,
            },
            on_hit_response_rules: OnHitResponseRules::Run,
            truncated_udp: CompiledTruncatedUdp {
                enabled: false,
                ttl_secs: 60,
            },
            rotate_rrset_on_serve: false,
            memory: CompiledMemoryCache {
                shard_count: 1,
                eviction: EvictionMode::Passive,
            },
            lmdb: Some(conduit_config::lookup::CompiledLmdbCache {
                path,
                map_size_bytes,
                when_full: conduit_config::lookup::LmdbWhenFull::EvictOne,
                sample_size: 16,
                shard_count,
                lookup_thread_count: 1,
            }),
            max_entries: 1000,
        }
    }

    #[test]
    fn reconcile_lmdb_path_change_does_not_migrate_entries() {
        let dir = tempfile::tempdir().unwrap();
        let path_a = dir.path().join("a");
        let path_b = dir.path().join("b");
        let mut prev_map = HashMap::new();
        prev_map.insert(
            "durable".into(),
            lmdb_cache_instance(path_a, 2 * 1024 * 1024),
        );
        let registry = LookupCacheRegistry::from_snapshot(&prev_map);

        let txn = txn_for("path-reopen.example.", 1);
        let key = build_query_key(&txn).unwrap();
        let gate = registry.instance_gate("durable", &key).unwrap();
        registry.fill_from_forward(
            "durable",
            &key,
            &gate,
            nxdomain_wire("path-reopen.example."),
            &txn,
        );
        assert_eq!(registry.entry_count("durable"), 1);
        assert!(matches!(
            registry.lookup("durable", &txn, Instant::now()),
            CacheLookupOutcome::Hit { .. }
        ));

        let mut new_map = HashMap::new();
        new_map.insert(
            "durable".into(),
            lmdb_cache_instance(path_b, 2 * 1024 * 1024),
        );
        registry.reconcile(&prev_map, &new_map).unwrap();

        // New path is empty — miss (no automatic migration).
        assert!(matches!(
            registry.lookup("durable", &txn, Instant::now()),
            CacheLookupOutcome::Miss { .. }
        ));
        assert_eq!(registry.entry_count("durable"), 0);
    }

    #[test]
    fn reconcile_lmdb_path_change_failure_keeps_prior_backend() {
        let dir = tempfile::tempdir().unwrap();
        let path_a = dir.path().join("a");
        let mut prev_map = HashMap::new();
        prev_map.insert(
            "durable".into(),
            lmdb_cache_instance(path_a, 2 * 1024 * 1024),
        );
        let registry = LookupCacheRegistry::from_snapshot(&prev_map);

        let txn = txn_for("path-fail.example.", 2);
        let key = build_query_key(&txn).unwrap();
        let gate = registry.instance_gate("durable", &key).unwrap();
        registry.fill_from_forward(
            "durable",
            &key,
            &gate,
            nxdomain_wire("path-fail.example."),
            &txn,
        );
        assert!(matches!(
            registry.lookup("durable", &txn, Instant::now()),
            CacheLookupOutcome::Hit { .. }
        ));

        let bad_file = dir.path().join("not-a-dir");
        std::fs::write(&bad_file, b"x").unwrap();
        let mut new_map = HashMap::new();
        // Path exists as a regular file — open must fail.
        new_map.insert(
            "durable".into(),
            lmdb_cache_instance(bad_file, 2 * 1024 * 1024),
        );
        let err = registry.reconcile(&prev_map, &new_map).unwrap_err();
        assert!(
            err.contains("failed to open") || err.contains("not a directory"),
            "unexpected error: {err}"
        );

        // Prior env still serves.
        assert!(matches!(
            registry.lookup("durable", &txn, Instant::now()),
            CacheLookupOutcome::Hit { .. }
        ));
        assert_eq!(registry.entry_count("durable"), 1);
    }

    #[test]
    fn reconcile_lmdb_map_size_grow_and_shrink_empty() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("env");
        let mut prev_map = HashMap::new();
        prev_map.insert(
            "durable".into(),
            lmdb_cache_instance(path.clone(), 2 * 1024 * 1024),
        );
        let registry = LookupCacheRegistry::from_snapshot(&prev_map);

        let mut grown = prev_map.clone();
        grown
            .get_mut("durable")
            .unwrap()
            .lmdb
            .as_mut()
            .unwrap()
            .map_size_bytes = 4 * 1024 * 1024;
        registry.reconcile(&prev_map, &grown).unwrap();
        assert_eq!(
            registry
                .instance("durable")
                .unwrap()
                .backend
                .load()
                .as_lmdb()
                .unwrap()
                .map_size_bytes(),
            4 * 1024 * 1024
        );

        let mut shrunk = grown.clone();
        shrunk
            .get_mut("durable")
            .unwrap()
            .lmdb
            .as_mut()
            .unwrap()
            .map_size_bytes = 2 * 1024 * 1024;
        registry.reconcile(&grown, &shrunk).unwrap();
        assert_eq!(
            registry
                .instance("durable")
                .unwrap()
                .backend
                .load()
                .as_lmdb()
                .unwrap()
                .map_size_bytes(),
            2 * 1024 * 1024
        );
    }

    #[test]
    fn reconcile_removes_lmdb_instance_keeps_files() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("env");
        let mut prev_map = HashMap::new();
        prev_map.insert(
            "durable".into(),
            lmdb_cache_instance(path.clone(), 2 * 1024 * 1024),
        );
        let registry = LookupCacheRegistry::from_snapshot(&prev_map);
        let txn = txn_for("drop-keep.example.", 3);
        let key = build_query_key(&txn).unwrap();
        let gate = registry.instance_gate("durable", &key).unwrap();
        registry.fill_from_forward(
            "durable",
            &key,
            &gate,
            nxdomain_wire("drop-keep.example."),
            &txn,
        );

        let new_map = HashMap::new();
        registry.reconcile(&prev_map, &new_map).unwrap();
        assert!(registry.instance("durable").is_none());
        assert!(
            path.exists(),
            "LMDB files must remain on disk after instance removal"
        );
    }

    #[test]
    fn reconcile_lmdb_shard_count_change_does_not_migrate_entries() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("env");
        let mut prev_map = HashMap::new();
        prev_map.insert(
            "durable".into(),
            lmdb_cache_instance_shards(path.clone(), 2 * 1024 * 1024, Some(2)),
        );
        let registry = LookupCacheRegistry::from_snapshot(&prev_map);

        let txn = txn_for("shard-reopen.example.", 1);
        let key = build_query_key(&txn).unwrap();
        let gate = registry.instance_gate("durable", &key).unwrap();
        registry.fill_from_forward(
            "durable",
            &key,
            &gate,
            nxdomain_wire("shard-reopen.example."),
            &txn,
        );
        assert_eq!(registry.entry_count("durable"), 1);
        assert_eq!(
            registry
                .instance("durable")
                .unwrap()
                .backend
                .load()
                .as_lmdb()
                .unwrap()
                .shard_count(),
            2
        );

        let mut new_map = HashMap::new();
        new_map.insert(
            "durable".into(),
            lmdb_cache_instance_shards(path.clone(), 2 * 1024 * 1024, Some(4)),
        );
        registry.reconcile(&prev_map, &new_map).unwrap();

        assert!(matches!(
            registry.lookup("durable", &txn, Instant::now()),
            CacheLookupOutcome::Miss { .. }
        ));
        assert_eq!(registry.entry_count("durable"), 0);
        assert_eq!(
            registry
                .instance("durable")
                .unwrap()
                .backend
                .load()
                .as_lmdb()
                .unwrap()
                .shard_count(),
            4
        );
        assert_eq!(
            registry
                .instance("durable")
                .unwrap()
                .backend
                .load()
                .as_lmdb()
                .unwrap()
                .path(),
            path
        );
    }

    #[test]
    fn reconcile_lmdb_shard_count_open_failure_keeps_prior() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("env");
        let mut prev_map = HashMap::new();
        prev_map.insert(
            "durable".into(),
            lmdb_cache_instance_shards(path.clone(), 2 * 1024 * 1024, Some(2)),
        );
        let registry = LookupCacheRegistry::from_snapshot(&prev_map);

        let txn = txn_for("shard-fail.example.", 2);
        let key = build_query_key(&txn).unwrap();
        let gate = registry.instance_gate("durable", &key).unwrap();
        registry.fill_from_forward(
            "durable",
            &key,
            &gate,
            nxdomain_wire("shard-fail.example."),
            &txn,
        );
        assert!(matches!(
            registry.lookup("durable", &txn, Instant::now()),
            CacheLookupOutcome::Hit { .. }
        ));

        // Occupy the staging sibling path with a regular file so open_for_shard_reopen fails.
        let mut staging = path.as_os_str().to_owned();
        staging.push(".conduit-lmdb-reopen");
        let staging = std::path::PathBuf::from(staging);
        std::fs::write(&staging, b"not-a-directory").unwrap();

        let mut new_map = HashMap::new();
        new_map.insert(
            "durable".into(),
            lmdb_cache_instance_shards(path.clone(), 2 * 1024 * 1024, Some(8)),
        );
        let err = registry.reconcile(&prev_map, &new_map).unwrap_err();
        assert!(
            err.contains("failed to open") || err.contains("shard"),
            "unexpected error: {err}"
        );

        assert!(matches!(
            registry.lookup("durable", &txn, Instant::now()),
            CacheLookupOutcome::Hit { .. }
        ));
        assert_eq!(registry.entry_count("durable"), 1);
        assert_eq!(
            registry
                .instance("durable")
                .unwrap()
                .backend
                .load()
                .as_lmdb()
                .unwrap()
                .shard_count(),
            2
        );
        // Prior on-disk files remain.
        assert!(path.join("conduit-lmdb-shards.json").exists() || path.join("shard-0").exists());
    }

    #[test]
    fn reconcile_omit_shard_count_keeps_live_layout() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("env");
        let mut prev_map = HashMap::new();
        prev_map.insert(
            "durable".into(),
            lmdb_cache_instance_shards(path.clone(), 2 * 1024 * 1024, Some(4)),
        );
        let registry = LookupCacheRegistry::from_snapshot(&prev_map);
        let txn = txn_for("omit-keep.example.", 4);
        let key = build_query_key(&txn).unwrap();
        let gate = registry.instance_gate("durable", &key).unwrap();
        registry.fill_from_forward(
            "durable",
            &key,
            &gate,
            nxdomain_wire("omit-keep.example."),
            &txn,
        );

        let mut new_map = HashMap::new();
        // Omit shard_count but bump lookup_thread_count — must not abandon.
        let mut cfg = lmdb_cache_instance_shards(path, 2 * 1024 * 1024, None);
        cfg.lmdb.as_mut().unwrap().lookup_thread_count = 32;
        new_map.insert("durable".into(), cfg);
        registry.reconcile(&prev_map, &new_map).unwrap();

        assert!(matches!(
            registry.lookup("durable", &txn, Instant::now()),
            CacheLookupOutcome::Hit { .. }
        ));
        assert_eq!(
            registry
                .instance("durable")
                .unwrap()
                .backend
                .load()
                .as_lmdb()
                .unwrap()
                .shard_count(),
            4
        );
    }
}
