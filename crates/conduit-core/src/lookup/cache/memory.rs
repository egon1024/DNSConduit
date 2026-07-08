//! Sharded in-memory cache backend (hash routes shard; full key equality).

use super::entry::{expires_at_from_ttl, CacheEntry, EntryKind};
use super::key::CacheKey;
use conduit_config::lookup::{CompiledCacheInstance, EvictionMode};
use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

pub struct MemoryCacheBackend {
    shard_count: usize,
    #[allow(dead_code)]
    eviction: EvictionMode,
    max_entries: u64,
    shards: Vec<RwLock<Shard>>,
}

struct Shard {
    entries: HashMap<CacheKey, CacheEntry>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CacheGetResult {
    Hit,
    Miss,
    Expired,
}

impl MemoryCacheBackend {
    pub fn from_config(cfg: &CompiledCacheInstance) -> Self {
        let shard_count = cfg.memory.shard_count.max(1) as usize;
        let mut shards = Vec::with_capacity(shard_count);
        for _ in 0..shard_count {
            shards.push(RwLock::new(Shard {
                entries: HashMap::new(),
            }));
        }
        Self {
            shard_count,
            eviction: cfg.memory.eviction,
            max_entries: cfg.max_entries,
            shards,
        }
    }

    fn shard_index(&self, key: &CacheKey) -> usize {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        key.0.hash(&mut hasher);
        (hasher.finish() as usize) % self.shard_count
    }

    pub fn get(&self, key: &CacheKey, now: Instant) -> Option<CacheEntry> {
        let shard = &self.shards[self.shard_index(key)];
        let guard = shard.read();
        let entry = guard.entries.get(key)?;
        if entry.is_fresh(now) {
            Some(entry.clone())
        } else {
            None
        }
    }

    pub fn get_result(&self, key: &CacheKey, now: Instant) -> CacheGetResult {
        let shard = &self.shards[self.shard_index(key)];
        let guard = shard.read();
        match guard.entries.get(key) {
            None => CacheGetResult::Miss,
            Some(entry) if entry.is_fresh(now) => CacheGetResult::Hit,
            Some(_) => CacheGetResult::Miss,
        }
    }

    pub fn insert(&self, key: CacheKey, entry: CacheEntry, now: Instant) {
        let idx = self.shard_index(&key);
        let mut guard = self.shards[idx].write();
        if let CacheGetResult::Expired = guard
            .entries
            .get(&key)
            .map(|e| {
                if e.is_fresh(now) {
                    CacheGetResult::Hit
                } else {
                    CacheGetResult::Expired
                }
            })
            .unwrap_or(CacheGetResult::Miss)
        {
            guard.entries.remove(&key);
        }
        if self.max_entries > 0 {
            self.evict_if_needed(&mut guard, idx, now);
        }
        guard.entries.insert(key, entry);
    }

    fn evict_if_needed(&self, shard: &mut Shard, shard_idx: usize, now: Instant) {
        if self.max_entries == 0 {
            return;
        }
        shard.entries.retain(|_, e| e.is_fresh(now));
        let mut total = shard.entries.len() as u64;
        for (i, s) in self.shards.iter().enumerate() {
            if i == shard_idx {
                continue;
            }
            total += s.read().entries.len() as u64;
        }
        if total < self.max_entries {
            return;
        }
        // Passive evict-on-insert: drop one expired or arbitrary entry in this shard.
        if let Some(stale_key) = shard
            .entries
            .iter()
            .find(|(_, e)| !e.is_fresh(now))
            .map(|(k, _)| k.clone())
        {
            shard.entries.remove(&stale_key);
            return;
        }
        if let Some(first) = shard.entries.keys().next().cloned() {
            shard.entries.remove(&first);
        }
    }

    pub fn entry_count(&self) -> u64 {
        self.shards
            .iter()
            .map(|s| s.read().entries.len() as u64)
            .sum()
    }
}

/// Derive TTL seconds for a stored response wire.
pub fn ttl_for_wire(
    wire: &[u8],
    _kind: EntryKind,
    negative_enabled: bool,
    servfail_ttl_secs: u32,
) -> Option<u32> {
    use hickory_proto::op::Message;
    let msg = Message::from_vec(wire).ok()?;
    let rcode = msg.response_code().low() as u16;
    let answer_count = u16::try_from(msg.answers().len()).unwrap_or(u16::MAX);
    let kind = EntryKind::from_rcode(rcode, answer_count);

    match kind {
        EntryKind::ServFail => {
            if servfail_ttl_secs == 0 {
                None
            } else {
                Some(servfail_ttl_secs)
            }
        }
        EntryKind::NxDomain | EntryKind::NoData => {
            if !negative_enabled {
                None
            } else {
                cacheable_ttl_from_rrset(minimum_ttl_from_message(&msg), 300)
            }
        }
        EntryKind::Positive => cacheable_ttl_from_rrset(minimum_ttl_from_message(&msg), 60),
    }
}

/// RFC 2181: TTL 0 means the RR must not be cached; omit storage instead of
/// inserting an entry that expires immediately.
fn cacheable_ttl_from_rrset(min_ttl: Option<u32>, default_if_absent: u32) -> Option<u32> {
    match min_ttl {
        None => Some(default_if_absent),
        Some(0) => None,
        Some(t) => Some(t),
    }
}

fn minimum_ttl_from_message(msg: &hickory_proto::op::Message) -> Option<u32> {
    let mut min_ttl: Option<u32> = None;
    for rr in msg.answers().iter().chain(msg.name_servers().iter()) {
        let ttl = rr.ttl();
        min_ttl = Some(min_ttl.map_or(ttl, |m| m.min(ttl)));
    }
    min_ttl
}

pub fn entry_from_wire(
    wire: Arc<[u8]>,
    negative_enabled: bool,
    servfail_ttl_secs: u32,
    now: Instant,
) -> Option<CacheEntry> {
    use hickory_proto::op::Message;
    let msg = Message::from_vec(&wire).ok()?;
    let rcode = msg.response_code().low() as u16;
    let answer_count = u16::try_from(msg.answers().len()).unwrap_or(u16::MAX);
    let kind = EntryKind::from_rcode(rcode, answer_count);
    let ttl = ttl_for_wire(&wire, kind, negative_enabled, servfail_ttl_secs)?;
    Some(CacheEntry {
        kind,
        wire,
        filled_at: now,
        expires_at: expires_at_from_ttl(now, ttl),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use conduit_config::lookup::{
        CacheBackendType, CompiledCacheInstance, CompiledMemoryCache, CompiledNegativeCache,
        CompiledTruncatedUdp, EvictionMode, OnHitResponseRules,
    };

    fn test_instance(max_entries: u64) -> CompiledCacheInstance {
        CompiledCacheInstance {
            name: "test".into(),
            backend_type: CacheBackendType::Memory,
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
                shard_count: 4,
                eviction: EvictionMode::Passive,
            },
            max_entries,
        }
    }

    fn sample_wire() -> Arc<[u8]> {
        use hickory_proto::op::{Message, Query};
        use hickory_proto::rr::{Name, RData, Record, RecordType};
        use hickory_proto::serialize::binary::{BinEncodable, BinEncoder};
        let name = Name::from_utf8("example.com.").unwrap();
        let mut msg = Message::new();
        msg.add_query(Query::query(name.clone(), RecordType::A));
        msg.add_answer(Record::from_rdata(
            name,
            120,
            RData::A(hickory_proto::rr::rdata::A(std::net::Ipv4Addr::new(
                1, 2, 3, 4,
            ))),
        ));
        let mut buf = Vec::new();
        let mut enc = BinEncoder::new(&mut buf);
        msg.emit(&mut enc).unwrap();
        buf.into()
    }

    #[test]
    fn hit_and_miss() {
        let backend = MemoryCacheBackend::from_config(&test_instance(0));
        let key = CacheKey(b"test-key".to_vec());
        let now = Instant::now();
        assert_eq!(backend.get_result(&key, now), CacheGetResult::Miss);

        let wire = sample_wire();
        let entry = entry_from_wire(wire.clone(), true, 10, now).unwrap();
        backend.insert(key.clone(), entry, now);
        assert_eq!(backend.get_result(&key, now), CacheGetResult::Hit);
        assert_eq!(backend.get(&key, now).unwrap().wire, wire);
    }

    #[test]
    fn distinct_keys_same_hash_bucket_coexist() {
        let backend = MemoryCacheBackend::from_config(&test_instance(0));
        let now = Instant::now();
        let k1 = CacheKey(b"key-alpha".to_vec());
        let k2 = CacheKey(b"key-beta".to_vec());
        let w1 = sample_wire();
        let mut w2_vec = sample_wire().to_vec();
        w2_vec[0] ^= 0xff;
        let w2: Arc<[u8]> = w2_vec.into();

        backend.insert(
            k1.clone(),
            entry_from_wire(w1.clone(), true, 10, now).unwrap(),
            now,
        );
        backend.insert(
            k2.clone(),
            entry_from_wire(w2.clone(), true, 10, now).unwrap(),
            now,
        );

        assert_eq!(backend.get(&k1, now).unwrap().wire, w1);
        assert_eq!(backend.get(&k2, now).unwrap().wire, w2);
    }

    #[test]
    fn ttl_expiry() {
        let backend = MemoryCacheBackend::from_config(&test_instance(0));
        let key = CacheKey(b"expiring".to_vec());
        let now = Instant::now();
        let entry = CacheEntry {
            kind: EntryKind::Positive,
            wire: sample_wire(),
            filled_at: now - std::time::Duration::from_secs(3600),
            expires_at: now - std::time::Duration::from_secs(1),
        };
        backend.insert(key.clone(), entry, now);
        assert_eq!(backend.get_result(&key, now), CacheGetResult::Miss);
    }

    #[test]
    fn zero_answer_ttl_is_not_cached() {
        use hickory_proto::op::{Message, Query};
        use hickory_proto::rr::{Name, RData, Record, RecordType};
        use hickory_proto::serialize::binary::{BinEncodable, BinEncoder};
        let name = Name::from_utf8("zero-ttl.example.com.").unwrap();
        let mut msg = Message::new();
        msg.add_query(Query::query(name.clone(), RecordType::A));
        msg.add_answer(Record::from_rdata(
            name,
            0,
            RData::A(hickory_proto::rr::rdata::A(std::net::Ipv4Addr::new(
                192, 0, 2, 1,
            ))),
        ));
        let mut buf = Vec::new();
        let mut enc = BinEncoder::new(&mut buf);
        msg.emit(&mut enc).unwrap();
        let wire: Arc<[u8]> = buf.into();
        assert!(entry_from_wire(wire, true, 10, Instant::now()).is_none());
    }

    #[test]
    fn insert_with_max_entries_does_not_deadlock() {
        let backend = MemoryCacheBackend::from_config(&test_instance(100));
        let key = CacheKey(b"bounded-cache-key".to_vec());
        let now = Instant::now();
        let entry = entry_from_wire(sample_wire(), true, 10, now).unwrap();
        backend.insert(key.clone(), entry, now);
        assert_eq!(backend.get_result(&key, now), CacheGetResult::Hit);
        assert_eq!(backend.entry_count(), 1);
    }

    #[test]
    fn servfail_knob_zero_disables_storage() {
        use hickory_proto::op::Message;
        use hickory_proto::serialize::binary::{BinEncodable, BinEncoder};
        let mut msg = Message::new();
        msg.set_response_code(hickory_proto::op::ResponseCode::ServFail);
        let mut buf = Vec::new();
        let mut enc = BinEncoder::new(&mut buf);
        msg.emit(&mut enc).unwrap();
        let wire: Arc<[u8]> = buf.into();
        assert!(entry_from_wire(wire.clone(), true, 0, Instant::now()).is_none());
        assert!(entry_from_wire(wire, true, 10, Instant::now()).is_some());
    }
}
