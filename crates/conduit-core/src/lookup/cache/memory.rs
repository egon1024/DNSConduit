//! Sharded in-memory cache backend (hash routes shard; full key equality).

use super::entry::{expires_at_from_ttl, CacheEntry, EntryKind};
use super::key::CacheKey;
use conduit_config::lookup::CompiledCacheInstance;
use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Bounds for one write-lock acquisition during active eviction reaping.
///
/// Defaults are fixed today; a future OpenSpec may map operator knobs onto this
/// struct (e.g. `memory.reap_max_lock_hold_ms`, `memory.reap_max_keys_per_lock`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReapBudget {
    /// Maximum wall time to hold a single shard write lock.
    pub max_lock_hold: Duration,
    /// Maximum expired entries removed under one shard write lock.
    pub max_keys_per_lock: usize,
}

impl ReapBudget {
    /// Production defaults for the active reaper (not operator-configurable yet).
    pub const DEFAULT: Self = Self {
        max_lock_hold: Duration::from_millis(1),
        // Cap expired removals per lock; scan may examine more fresh keys until max_lock_hold.
        max_keys_per_lock: 1024,
    };

    /// Effectively unbounded — used by tests that need a full pass in one call.
    pub const UNBOUNDED: Self = Self {
        max_lock_hold: Duration::from_secs(3600),
        max_keys_per_lock: usize::MAX,
    };
}

/// Round-robin resume point across shards between reaper ticks.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ReapCursor {
    pub next_shard: usize,
}

/// Result of one budgeted reap pass over the backend.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReapOutcome {
    pub removed: u64,
    /// True when a shard lock budget was exhausted before finishing that shard
    /// (caller should resume the same `cursor.next_shard` on the next tick).
    pub incomplete: bool,
}

pub struct MemoryCacheBackend {
    shard_count: usize,
    max_entries: AtomicU64,
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
            max_entries: AtomicU64::new(cfg.max_entries),
            shards,
        }
    }

    pub fn max_entries(&self) -> u64 {
        self.max_entries.load(Ordering::Relaxed)
    }

    /// Update the live entry cap without rebuilding shard maps. When the cap is lowered,
    /// evicts entries until the cache is at or under the new limit.
    pub fn set_max_entries(&self, max_entries: u64) {
        let prev = self.max_entries.swap(max_entries, Ordering::Relaxed);
        if max_entries > 0 && (prev > max_entries || prev == 0) && self.entry_count() > max_entries
        {
            self.trim_to_max(Instant::now());
        }
    }

    pub fn trim_to_max(&self, now: Instant) {
        let max = self.max_entries.load(Ordering::Relaxed);
        if max == 0 {
            return;
        }
        while self.entry_count() > max {
            if !self.evict_one(now) {
                break;
            }
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
        if self.max_entries.load(Ordering::Relaxed) > 0 {
            self.evict_if_needed(&mut guard, idx, now);
        }
        guard.entries.insert(key, entry);
    }

    /// Remove an entry by key if present. Returns `true` when an entry was removed.
    pub fn remove(&self, key: &CacheKey) -> bool {
        let idx = self.shard_index(key);
        let mut guard = self.shards[idx].write();
        guard.entries.remove(key).is_some()
    }

    fn evict_if_needed(&self, shard: &mut Shard, shard_idx: usize, now: Instant) {
        let max_entries = self.max_entries.load(Ordering::Relaxed);
        if max_entries == 0 {
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
        if total < max_entries {
            return;
        }
        Self::evict_one_in_shard(shard, now);
    }

    /// Remove one entry from any shard (stale first). Returns false when the cache is empty.
    fn evict_one(&self, now: Instant) -> bool {
        for shard in &self.shards {
            let mut guard = shard.write();
            if Self::evict_one_in_shard(&mut guard, now) {
                return true;
            }
        }
        false
    }

    fn evict_one_in_shard(shard: &mut Shard, now: Instant) -> bool {
        if let Some(stale_key) = shard
            .entries
            .iter()
            .find(|(_, e)| !e.is_fresh(now))
            .map(|(k, _)| k.clone())
        {
            shard.entries.remove(&stale_key);
            return true;
        }
        if let Some(first) = shard.entries.keys().next().cloned() {
            shard.entries.remove(&first);
            return true;
        }
        false
    }

    pub fn entry_count(&self) -> u64 {
        self.shards
            .iter()
            .map(|s| s.read().entries.len() as u64)
            .sum()
    }

    pub fn shard_count(&self) -> usize {
        self.shard_count
    }

    /// Drop expired entries across all shards with an unbounded budget.
    pub fn reap_expired(&self, now: Instant) -> u64 {
        let mut cursor = ReapCursor::default();
        self.reap_expired_budgeted(now, ReapBudget::UNBOUNDED, &mut cursor)
            .removed
    }

    /// Reap expired entries under per-lock time/key budgets, round-robin from `cursor`.
    ///
    /// Each shard gets at most one write-lock acquisition per call. If that
    /// acquisition hits `max_lock_hold` or `max_keys_per_lock` before the shard
    /// is fully scanned, the call stops and leaves `cursor.next_shard` on that
    /// shard so the next tick resumes there. Otherwise the cursor advances to
    /// the next shard after each completed shard; a full ring restores the
    /// starting shard index.
    pub fn reap_expired_budgeted(
        &self,
        now: Instant,
        budget: ReapBudget,
        cursor: &mut ReapCursor,
    ) -> ReapOutcome {
        let n = self.shard_count;
        if n == 0 {
            return ReapOutcome {
                removed: 0,
                incomplete: false,
            };
        }
        let start = cursor.next_shard % n;
        let mut removed = 0u64;
        for offset in 0..n {
            let idx = (start + offset) % n;
            let (shard_removed, finished) = self.reap_shard_budgeted(idx, now, budget);
            removed += shard_removed;
            if !finished {
                cursor.next_shard = idx;
                return ReapOutcome {
                    removed,
                    incomplete: true,
                };
            }
            cursor.next_shard = (idx + 1) % n;
        }
        ReapOutcome {
            removed,
            incomplete: false,
        }
    }

    /// Returns `(removed, finished_shard)`.
    fn reap_shard_budgeted(
        &self,
        shard_idx: usize,
        now: Instant,
        budget: ReapBudget,
    ) -> (u64, bool) {
        let lock_deadline = Instant::now() + budget.max_lock_hold;
        let mut guard = self.shards[shard_idx].write();
        let mut to_remove = Vec::new();
        let mut finished = true;

        for (examined, (key, entry)) in guard.entries.iter().enumerate() {
            // Sample the clock periodically to keep the hot loop cheap.
            if examined & 63 == 0 && Instant::now() >= lock_deadline {
                finished = false;
                break;
            }
            if !entry.is_fresh(now) {
                to_remove.push(key.clone());
                if to_remove.len() >= budget.max_keys_per_lock {
                    finished = false;
                    break;
                }
            }
        }

        let removed = to_remove.len() as u64;
        for key in to_remove {
            guard.entries.remove(&key);
        }
        (removed, finished)
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
            lmdb: None,
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

        assert!(backend.remove(&key));
        assert_eq!(backend.get_result(&key, now), CacheGetResult::Miss);
        assert!(!backend.remove(&key));
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
    fn reap_expired_removes_stale_leaves_fresh() {
        let backend = MemoryCacheBackend::from_config(&test_instance(0));
        let now = Instant::now();
        let fresh_key = CacheKey(b"fresh".to_vec());
        let stale_key = CacheKey(b"stale".to_vec());
        backend.insert(
            fresh_key.clone(),
            entry_from_wire(sample_wire(), true, 10, now).unwrap(),
            now,
        );
        backend.insert(
            stale_key.clone(),
            CacheEntry {
                kind: EntryKind::Positive,
                wire: sample_wire(),
                filled_at: now - std::time::Duration::from_secs(60),
                expires_at: now - std::time::Duration::from_secs(1),
            },
            now,
        );
        assert_eq!(backend.entry_count(), 2);

        let removed = backend.reap_expired(now);
        assert_eq!(removed, 1);
        assert_eq!(backend.entry_count(), 1);
        assert_eq!(backend.get_result(&fresh_key, now), CacheGetResult::Hit);
        assert_eq!(backend.get_result(&stale_key, now), CacheGetResult::Miss);
    }

    fn test_instance_shards(max_entries: u64, shard_count: u32) -> CompiledCacheInstance {
        let mut cfg = test_instance(max_entries);
        cfg.memory.shard_count = shard_count;
        cfg
    }

    fn stale_entry(now: Instant) -> CacheEntry {
        CacheEntry {
            kind: EntryKind::Positive,
            wire: sample_wire(),
            filled_at: now - std::time::Duration::from_secs(60),
            expires_at: now - std::time::Duration::from_secs(1),
        }
    }

    #[test]
    fn reap_budget_limits_removals_per_lock_and_resumes_same_shard() {
        let backend = MemoryCacheBackend::from_config(&test_instance_shards(0, 1));
        let now = Instant::now();
        for i in 0..5 {
            backend.insert(
                CacheKey(format!("stale-{i}").into_bytes()),
                stale_entry(now),
                now,
            );
        }
        assert_eq!(backend.entry_count(), 5);

        let budget = ReapBudget {
            max_lock_hold: Duration::from_secs(60),
            max_keys_per_lock: 2,
        };
        let mut cursor = ReapCursor::default();
        let first = backend.reap_expired_budgeted(now, budget, &mut cursor);
        assert_eq!(first.removed, 2);
        assert!(first.incomplete);
        assert_eq!(cursor.next_shard, 0);
        assert_eq!(backend.entry_count(), 3);

        let second = backend.reap_expired_budgeted(now, budget, &mut cursor);
        assert_eq!(second.removed, 2);
        assert!(second.incomplete);
        assert_eq!(backend.entry_count(), 1);

        let third = backend.reap_expired_budgeted(now, budget, &mut cursor);
        assert_eq!(third.removed, 1);
        assert!(!third.incomplete);
        assert_eq!(backend.entry_count(), 0);
    }

    #[test]
    fn reap_budget_advances_cursor_across_shards() {
        let backend = MemoryCacheBackend::from_config(&test_instance_shards(0, 2));
        let now = Instant::now();
        // Insert enough keys that both shards are likely non-empty.
        for i in 0..32 {
            backend.insert(
                CacheKey(format!("stale-{i}").into_bytes()),
                stale_entry(now),
                now,
            );
        }
        let before = backend.entry_count();
        assert!(before > 0);

        let budget = ReapBudget::UNBOUNDED;
        let mut cursor = ReapCursor { next_shard: 0 };
        let outcome = backend.reap_expired_budgeted(now, budget, &mut cursor);
        assert!(!outcome.incomplete);
        assert_eq!(outcome.removed, before);
        assert_eq!(backend.entry_count(), 0);
        assert_eq!(cursor.next_shard, 0, "full ring restores start shard");
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
    fn set_max_entries_lowers_cap_and_trims_live_entries() {
        let backend = MemoryCacheBackend::from_config(&test_instance(0));
        let now = Instant::now();
        for i in 0..5 {
            let key = CacheKey(format!("key-{i}").into_bytes());
            backend.insert(
                key,
                entry_from_wire(sample_wire(), true, 10, now).unwrap(),
                now,
            );
        }
        assert_eq!(backend.entry_count(), 5);

        backend.set_max_entries(2);
        assert_eq!(backend.max_entries(), 2);
        assert_eq!(backend.entry_count(), 2);
    }

    #[test]
    fn set_max_entries_raise_does_not_evict() {
        let backend = MemoryCacheBackend::from_config(&test_instance(2));
        let now = Instant::now();
        for i in 0..2 {
            let key = CacheKey(format!("key-{i}").into_bytes());
            backend.insert(
                key,
                entry_from_wire(sample_wire(), true, 10, now).unwrap(),
                now,
            );
        }
        assert_eq!(backend.entry_count(), 2);

        backend.set_max_entries(100);
        assert_eq!(backend.entry_count(), 2);
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
