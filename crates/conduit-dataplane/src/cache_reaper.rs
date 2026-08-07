//! Background reaper for memory caches with `memory.eviction: active`.

use crate::listener::DataplaneShutdown;
use conduit_core::lookup::{LookupCacheRegistry, ReapBudget};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

/// Fixed interval for the active-eviction reaper (no operator knob in this release).
pub const ACTIVE_CACHE_REAP_INTERVAL: Duration = Duration::from_secs(5);

/// Spawn a background thread that periodically drops expired entries for
/// instances with `memory.eviction: active`. Returns `None` when no such
/// instance exists (eviction mode requires restart to change).
///
/// Each tick uses [`ReapBudget::DEFAULT`]: at most ~1ms write-lock hold per shard
/// and at most 1024 expired removals per lock, round-robin across shards so a
/// large cache cannot stall the datapath for a full map scan.
pub fn spawn_cache_reaper(
    cache: Arc<LookupCacheRegistry>,
    shutdown: DataplaneShutdown,
) -> Option<thread::JoinHandle<()>> {
    if !cache.has_active_eviction() {
        return None;
    }
    tracing::info!(
        interval_secs = ACTIVE_CACHE_REAP_INTERVAL.as_secs(),
        max_lock_hold_ms = ReapBudget::DEFAULT.max_lock_hold.as_millis() as u64,
        max_keys_per_lock = ReapBudget::DEFAULT.max_keys_per_lock,
        "active cache eviction reaper starting"
    );
    Some(thread::spawn(move || {
        while !shutdown.is_shutdown() {
            let removed = cache.reap_active_expired(Instant::now());
            if removed > 0 {
                tracing::debug!(removed, "active cache reaper trimmed expired entries");
            }
            // Sleep in short slices so shutdown is noticed promptly.
            let deadline = Instant::now() + ACTIVE_CACHE_REAP_INTERVAL;
            while !shutdown.is_shutdown() && Instant::now() < deadline {
                let remaining = deadline.saturating_duration_since(Instant::now());
                let slice = remaining.min(Duration::from_millis(100));
                if slice.is_zero() {
                    break;
                }
                thread::sleep(slice);
            }
        }
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use conduit_config::lookup::{
        CacheBackendType, CompiledCacheInstance, CompiledMemoryCache, CompiledNegativeCache,
        CompiledTruncatedUdp, EvictionMode, OnHitResponseRules,
    };
    use std::collections::HashMap;

    fn instance(name: &str, eviction: EvictionMode) -> CompiledCacheInstance {
        CompiledCacheInstance {
            name: name.into(),
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
                shard_count: 2,
                eviction,
            },
            lmdb: None,
            max_entries: 0,
        }
    }

    #[test]
    fn spawn_skipped_when_only_passive() {
        let mut map = HashMap::new();
        map.insert("p".into(), instance("p", EvictionMode::Passive));
        let cache = Arc::new(LookupCacheRegistry::from_snapshot(&map));
        assert!(spawn_cache_reaper(cache, DataplaneShutdown::new()).is_none());
    }

    #[test]
    fn spawn_starts_when_active_present() {
        let mut map = HashMap::new();
        map.insert("a".into(), instance("a", EvictionMode::Active));
        let cache = Arc::new(LookupCacheRegistry::from_snapshot(&map));
        let shutdown = DataplaneShutdown::new();
        let handle = spawn_cache_reaper(cache, shutdown.clone()).expect("reaper thread");
        shutdown.signal();
        handle.join().expect("reaper join");
    }
}
