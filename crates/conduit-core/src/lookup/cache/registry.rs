//! Runtime cache store registry (outside snapshot; shared across workers).

use super::entry::{CacheEntry, EntryKind};
use super::inflight::{InFlightRole, InFlightTable};
use super::key::{
    build_key_from_parts, build_query_key, build_truncated_udp_key, CacheKey, TransportKey,
};
use super::memory::entry_from_wire;
use super::memory::MemoryCacheBackend;
use super::serve::prepare_served_arc;
use conduit_config::lookup::CompiledCacheInstance;
use conduit_metrics::MetricsHub;
use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
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
    if !inst.config.negative_cache.enabled
        || !inst.config.negative_cache.nxdomain_covers_descendants
    {
        return None;
    }
    let qname = txn.qname.as_deref().unwrap_or(".");
    for ancestor in ancestor_qnames(qname) {
        let key = build_key_from_parts(
            &ancestor,
            txn.qtype.unwrap_or(0),
            txn.qclass.unwrap_or(1),
            &txn.query_wire,
            TransportKey::Complete,
        )
        .ok()?;
        let entry = inst.backend.get(&key, now)?;
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

    pub fn apply_max_entries(&self, max_entries: u64) {
        self.backend.set_max_entries(max_entries);
    }

    pub fn max_entries(&self) -> u64 {
        self.backend.max_entries()
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

    /// Reconcile named cache instances after a snapshot swap: add new instances and
    /// apply live `max_entries` updates to existing backends without rebuilding shards.
    pub fn reconcile(
        &self,
        prev: &HashMap<String, CompiledCacheInstance>,
        new: &HashMap<String, CompiledCacheInstance>,
    ) {
        let mut guard = self.instances.write();
        for (name, cfg) in new {
            match guard.get(name) {
                Some(inst) => {
                    if prev.get(name).map(|p| p.max_entries) != Some(cfg.max_entries) {
                        inst.apply_max_entries(cfg.max_entries);
                    }
                }
                None => {
                    guard.insert(
                        name.clone(),
                        Arc::new(CacheInstanceRuntime::new(cfg.clone())),
                    );
                }
            }
        }
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

        if inst.config.truncated_udp.enabled
            && txn.protocol == crate::transaction::ClientProtocol::Udp
        {
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
                if let Some(entry) = inst.backend.get(&key, now) {
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
            Some(&txn.query_wire),
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
            Some(&txn.query_wire),
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
            tracing::warn!(
                cache = cache_name,
                "cache fill skipped: unknown cache instance at runtime"
            );
            return;
        };
        let now = Instant::now();

        let store_key = if should_store_truncated(&inst.config, &wire, txn) {
            build_truncated_udp_key(txn)
                .map(|k| (k, inst.config.truncated_udp.ttl_secs))
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
            inst.config.negative_cache.enabled,
            inst.config.negative_cache.servfail_ttl_secs,
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
        inst.backend.insert(insert_key, entry, now);
        if !storing_truncated {
            if let Ok(tc_key) = build_truncated_udp_key(txn) {
                if inst.backend.remove(&tc_key) {
                    tracing::debug!(
                        cache = cache_name,
                        "removed truncated UDP sibling after complete cache fill"
                    );
                }
            }
        }
        self.record_cache_fill(cache_name, txn);
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
            .map(|i| i.backend.entry_count())
            .unwrap_or(0)
    }

    pub fn max_entries(&self, cache_name: &str) -> Option<u64> {
        self.instance(cache_name).map(|i| i.max_entries())
    }

    pub fn all_entry_counts(&self) -> Vec<(String, u64)> {
        self.instances
            .read()
            .iter()
            .map(|(name, inst)| (name.clone(), inst.backend.entry_count()))
            .collect()
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
        hub.builtin.record_cache_fill(cache_name, profile);
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
        hub.builtin
            .record_cache_singleflight_coalesced(cache_name, profile);
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
            inst.backend.get(&tc_key, Instant::now()).is_none(),
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
        registry.reconcile(&prev_map, &new_map);

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
        registry.reconcile(&prev_map, &new_map);

        assert_eq!(registry.entry_count("global"), 1);
        assert_eq!(registry.entry_count("regional"), 0);
    }
}
