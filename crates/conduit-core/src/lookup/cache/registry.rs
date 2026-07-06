//! Runtime cache store registry (outside snapshot; shared across workers).

use super::entry::CacheEntry;
use super::inflight::{InFlightRole, InFlightTable};
use super::key::{build_query_key, build_truncated_udp_key, CacheKey};
use super::memory::entry_from_wire;
use super::memory::MemoryCacheBackend;
use super::serve::prepare_served_arc;
use conduit_config::lookup::CompiledCacheInstance;
use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

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
    pub config: CompiledCacheInstance,
    backend: MemoryCacheBackend,
    inflight: InFlightTable,
}

impl CacheInstanceRuntime {
    pub fn new(config: CompiledCacheInstance) -> Self {
        let backend = MemoryCacheBackend::from_config(&config);
        Self {
            config,
            backend,
            inflight: InFlightTable::new(),
        }
    }
}

pub struct LookupCacheRegistry {
    instances: RwLock<HashMap<String, Arc<CacheInstanceRuntime>>>,
    async_coalesce: bool,
    wake: RwLock<Option<CacheWaitWake>>,
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
            async_coalesce: false,
            wake: RwLock::new(None),
        }
    }

    pub fn reconcile(&self, instances: &HashMap<String, CompiledCacheInstance>) {
        let mut guard = self.instances.write();
        for (name, cfg) in instances {
            guard
                .entry(name.clone())
                .or_insert_with(|| Arc::new(CacheInstanceRuntime::new(cfg.clone())));
        }
    }

    pub fn set_async_coalesce(&mut self, enabled: bool) {
        self.async_coalesce = enabled;
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

        if inst.config.cache_truncated_udp {
            if let Ok(tc_key) = build_truncated_udp_key(txn) {
                if let Some(hit) = self.try_read_hit(&inst, &tc_key, txn, now) {
                    return hit;
                }
            }
        }

        let gate = inst.inflight.gate_for(&key);
        let role = gate.register(txn.id, self.async_coalesce);
        match role {
            InFlightRole::Leader => CacheLookupOutcome::Miss { key, gate },
            InFlightRole::Follower if self.async_coalesce => CacheLookupOutcome::WaitAsync { key },
            InFlightRole::Follower => {
                let _wire = gate.wait_for_result(Duration::from_secs(30));
                inst.inflight.remove(&key);
                let now = Instant::now();
                if let Some(entry) = inst.backend.get(&key, now) {
                    return self.hit_from_entry(&inst, &entry, txn, now);
                }
                if let Some(w) = _wire {
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
        let entry = inst.backend.get(key, now)?;
        Some(self.hit_from_entry(inst, &entry, txn, now))
    }

    fn hit_from_stored(
        &self,
        inst: &CacheInstanceRuntime,
        wire: &Arc<[u8]>,
        txn: &crate::transaction::Transaction,
        now: Instant,
    ) -> CacheLookupOutcome {
        // Coalesced follower when fill did not store (uncacheable wire): serve with zero age.
        let wire = match prepare_served_arc(
            wire,
            txn.dns_id,
            inst.config.rotate_rrset_on_serve,
            now,
            now,
        ) {
            Ok(w) => w,
            Err(e) => {
                tracing::warn!(
                    cache = inst.config.name,
                    error = %e,
                    "failed to prepare coalesced wire"
                );
                return CacheLookupOutcome::Bypass;
            }
        };
        CacheLookupOutcome::Hit {
            wire,
            cache_name: inst.config.name.clone(),
            skip_response_rules: matches!(
                inst.config.on_hit_response_rules,
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
        let wire = match prepare_served_arc(
            &entry.wire,
            txn.dns_id,
            inst.config.rotate_rrset_on_serve,
            entry.filled_at,
            now,
        ) {
            Ok(w) => w,
            Err(e) => {
                tracing::warn!(
                    cache = inst.config.name,
                    error = %e,
                    "failed to prepare cached wire"
                );
                return CacheLookupOutcome::Bypass;
            }
        };
        CacheLookupOutcome::Hit {
            wire,
            cache_name: inst.config.name.clone(),
            skip_response_rules: matches!(
                inst.config.on_hit_response_rules,
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
            return;
        };
        let now = Instant::now();

        let store_key = if should_store_truncated(&inst.config, &wire, txn) {
            build_truncated_udp_key(txn)
                .map(|k| (k, inst.config.truncated_udp_ttl_secs))
                .ok()
        } else {
            None
        };

        let (insert_key, override_ttl) = if let Some((k, ttl)) = store_key {
            (k, Some(ttl))
        } else {
            (key.clone(), None)
        };

        let mut entry = match entry_from_wire(
            wire.clone(),
            inst.config.negative_cache.enabled,
            inst.config.negative_cache.servfail_ttl_secs,
            now,
        ) {
            Some(e) => e,
            None => {
                let _ = gate.complete(None);
                inst.inflight.remove(key);
                return;
            }
        };

        if let Some(ttl) = override_ttl {
            entry.expires_at = super::entry::expires_at_from_ttl(now, ttl);
        }

        inst.backend.insert(insert_key, entry, now);
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
        if let Some(entry) = inst.backend.get(key, now) {
            inst.inflight.remove(key);
            return self.hit_from_entry(&inst, &entry, txn, now);
        }
        if let Some(wire) = gate.result() {
            inst.inflight.remove(key);
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
            .map(|i| i.backend.entry_count())
            .unwrap_or(0)
    }
}

fn should_store_truncated(
    cfg: &CompiledCacheInstance,
    wire: &[u8],
    txn: &crate::transaction::Transaction,
) -> bool {
    if !cfg.cache_truncated_udp {
        return false;
    }
    if txn.protocol != crate::transaction::ClientProtocol::Udp {
        return false;
    }
    hickory_proto::op::Message::from_vec(wire)
        .ok()
        .map(|m| m.header().truncated())
        .unwrap_or(false)
}
