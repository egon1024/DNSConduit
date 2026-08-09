//! Thin cache store backend (memory and LMDB).

use super::entry::CacheEntry;
use super::key::CacheKey;
use super::lmdb::{LmdbBackendError, LmdbCacheBackend};
use super::memory::{CacheGetResult, MemoryCacheBackend, ReapBudget, ReapCursor, ReapOutcome};
use conduit_config::lookup::{CacheBackendType, CompiledCacheInstance};
use std::time::Instant;

/// Result of a cache insert, including capacity-eviction cost when the store ejected victims.
#[derive(Debug, Clone, Copy, Default)]
pub struct InsertOutcome {
    /// `true` when the entry was stored (LMDB may refuse under pressure).
    pub stored: bool,
    /// Wall seconds spent in capacity eviction during this insert (sum if multiple).
    pub eviction_secs: f64,
    /// Number of capacity victims removed during this insert.
    pub evictions: u64,
}

impl InsertOutcome {
    pub fn stored_ok() -> Self {
        Self {
            stored: true,
            ..Self::default()
        }
    }

    pub fn refused() -> Self {
        Self::default()
    }
}

/// Process-local answer-cache store behind [`super::registry::CacheInstanceRuntime`].
pub enum CacheBackend {
    Memory(MemoryCacheBackend),
    Lmdb(LmdbCacheBackend),
}

impl CacheBackend {
    pub fn from_config(cfg: &CompiledCacheInstance) -> Result<Self, LmdbBackendError> {
        match cfg.backend_type {
            CacheBackendType::Memory => Ok(Self::Memory(MemoryCacheBackend::from_config(cfg))),
            CacheBackendType::Lmdb => Ok(Self::Lmdb(LmdbCacheBackend::open(cfg)?)),
            CacheBackendType::EbpfMap => {
                panic!(
                    "cache instance '{}': ebpf_map backend is reserved and not implemented",
                    cfg.name
                );
            }
        }
    }

    pub fn get(&self, key: &CacheKey, now: Instant) -> Option<CacheEntry> {
        match self {
            Self::Memory(m) => m.get(key, now),
            Self::Lmdb(l) => l.get(key, now),
        }
    }

    pub fn get_result(&self, key: &CacheKey, now: Instant) -> CacheGetResult {
        match self {
            Self::Memory(m) => m.get_result(key, now),
            Self::Lmdb(l) => l.get_result(key, now),
        }
    }

    /// Store an entry. LMDB may refuse under capacity pressure (`stored == false`).
    pub fn insert(&self, key: CacheKey, entry: CacheEntry, now: Instant) -> InsertOutcome {
        match self {
            Self::Memory(m) => m.insert(key, entry, now),
            Self::Lmdb(l) => l.insert(key, entry, now),
        }
    }

    pub fn remove(&self, key: &CacheKey) -> bool {
        match self {
            Self::Memory(m) => m.remove(key),
            Self::Lmdb(l) => l.remove(key),
        }
    }

    pub fn entry_count(&self) -> u64 {
        match self {
            Self::Memory(m) => m.entry_count(),
            Self::Lmdb(l) => l.entry_count(),
        }
    }

    pub fn max_entries(&self) -> u64 {
        match self {
            Self::Memory(m) => m.max_entries(),
            Self::Lmdb(l) => l.max_entries(),
        }
    }

    pub fn set_max_entries(&self, max_entries: u64) {
        match self {
            Self::Memory(m) => m.set_max_entries(max_entries),
            Self::Lmdb(l) => l.set_max_entries(max_entries),
        }
    }

    pub fn reap_expired_budgeted(
        &self,
        now: Instant,
        budget: ReapBudget,
        cursor: &mut ReapCursor,
    ) -> ReapOutcome {
        match self {
            Self::Memory(m) => m.reap_expired_budgeted(now, budget, cursor),
            // LMDB uses lazy expiry on read; no active reaper.
            Self::Lmdb(_) => ReapOutcome {
                removed: 0,
                incomplete: false,
            },
        }
    }

    pub fn as_lmdb(&self) -> Option<&LmdbCacheBackend> {
        match self {
            Self::Lmdb(l) => Some(l),
            Self::Memory(_) => None,
        }
    }
}
