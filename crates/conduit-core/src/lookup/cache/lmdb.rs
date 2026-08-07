//! LMDB durable answer-cache backend (`heed`, safe sync default).

use super::entry::{CacheEntry, EntryKind};
use super::key::CacheKey;
use super::memory::CacheGetResult;
use conduit_config::lookup::{CompiledCacheInstance, CompiledLmdbCache, LmdbWhenFull};
use heed::types::Bytes;
use heed::{Database, Env, EnvOpenOptions, MdbError};
use parking_lot::RwLock;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// On-disk format magic (`CDCL` = Conduit Cache LMDB).
const FORMAT_MAGIC: &[u8; 4] = b"CDCL";
/// Bump when the on-disk value layout changes; mismatched envs fail closed.
pub const FORMAT_VERSION: u32 = 1;

const META_DB: &str = "conduit_meta";
const ENTRIES_DB: &str = "conduit_entries";
const META_FORMAT_KEY: &[u8] = b"format";
const META_COUNT_KEY: &[u8] = b"entry_count";

/// Header size: filled_at_unix(8) + expires_at_unix(8) + kind(1) + wire_len(4).
const VALUE_HEADER_LEN: usize = 8 + 8 + 1 + 4;

#[derive(Debug, thiserror::Error)]
pub enum LmdbBackendError {
    #[error("failed to create LMDB directory '{path}': {source}")]
    CreateDir {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("failed to open LMDB environment at '{path}': {source}")]
    OpenEnv { path: PathBuf, source: heed::Error },
    #[error(
        "LMDB format mismatch at '{path}' (found version {found}, expected {expected}). \
         Move or delete the environment files and retry."
    )]
    FormatMismatch {
        path: PathBuf,
        found: u32,
        expected: u32,
    },
    #[error(
        "LMDB format magic mismatch at '{path}'. Move or delete the environment files and retry."
    )]
    FormatMagic { path: PathBuf },
    #[error("LMDB I/O error for cache '{cache}' at '{path}': {source}")]
    Io {
        cache: String,
        path: PathBuf,
        source: heed::Error,
    },
}

pub struct LmdbCacheBackend {
    cache_name: String,
    path: PathBuf,
    env: Env,
    meta: Database<Bytes, Bytes>,
    entries: Database<Bytes, Bytes>,
    max_entries: AtomicU64,
    map_size_bytes: AtomicU64,
    policy: RwLock<LmdbPolicy>,
}

#[derive(Debug, Clone, Copy)]
struct LmdbPolicy {
    when_full: LmdbWhenFull,
    sample_size: u32,
}

impl LmdbCacheBackend {
    pub fn open(cfg: &CompiledCacheInstance) -> Result<Self, LmdbBackendError> {
        let lmdb = cfg.lmdb.as_ref().ok_or_else(|| LmdbBackendError::Io {
            cache: cfg.name.clone(),
            path: PathBuf::new(),
            source: heed::Error::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "missing lmdb compiled config",
            )),
        })?;
        Self::open_at(&cfg.name, cfg.max_entries, lmdb)
    }

    pub fn open_at(
        cache_name: &str,
        max_entries: u64,
        lmdb: &CompiledLmdbCache,
    ) -> Result<Self, LmdbBackendError> {
        let path = lmdb.path.clone();
        // LMDB env path is a directory; create the leaf and any missing parents
        // (parent preflight at compile only checks ancestors when they already exist).
        if !path.exists() {
            std::fs::create_dir_all(&path).map_err(|source| LmdbBackendError::CreateDir {
                path: path.clone(),
                source,
            })?;
        } else if !path.is_dir() {
            return Err(LmdbBackendError::CreateDir {
                path: path.clone(),
                source: std::io::Error::other("lmdb.path exists and is not a directory"),
            });
        }

        let map_size = align_map_size(lmdb.map_size_bytes);
        // SAFETY: path is operator-controlled; we open each cache path once per process
        // for this instance (registry holds a single backend Arc per name).
        let env = unsafe {
            EnvOpenOptions::new()
                .map_size(map_size)
                .max_dbs(2)
                .open(&path)
        }
        .map_err(|source| LmdbBackendError::OpenEnv {
            path: path.clone(),
            source,
        })?;

        let mut wtxn = env.write_txn().map_err(|source| LmdbBackendError::Io {
            cache: cache_name.to_string(),
            path: path.clone(),
            source,
        })?;
        let meta: Database<Bytes, Bytes> =
            env.create_database(&mut wtxn, Some(META_DB))
                .map_err(|source| LmdbBackendError::Io {
                    cache: cache_name.to_string(),
                    path: path.clone(),
                    source,
                })?;
        let entries: Database<Bytes, Bytes> = env
            .create_database(&mut wtxn, Some(ENTRIES_DB))
            .map_err(|source| LmdbBackendError::Io {
                cache: cache_name.to_string(),
                path: path.clone(),
                source,
            })?;

        match meta.get(&wtxn, META_FORMAT_KEY) {
            Ok(Some(raw)) => {
                verify_format(&path, raw)?;
            }
            Ok(None) => {
                let mut buf = Vec::with_capacity(8);
                buf.extend_from_slice(FORMAT_MAGIC);
                buf.extend_from_slice(&FORMAT_VERSION.to_le_bytes());
                meta.put(&mut wtxn, META_FORMAT_KEY, &buf)
                    .map_err(|source| LmdbBackendError::Io {
                        cache: cache_name.to_string(),
                        path: path.clone(),
                        source,
                    })?;
                if meta.get(&wtxn, META_COUNT_KEY).ok().flatten().is_none() {
                    meta.put(&mut wtxn, META_COUNT_KEY, &0u64.to_le_bytes())
                        .map_err(|source| LmdbBackendError::Io {
                            cache: cache_name.to_string(),
                            path: path.clone(),
                            source,
                        })?;
                }
            }
            Err(source) => {
                return Err(LmdbBackendError::Io {
                    cache: cache_name.to_string(),
                    path: path.clone(),
                    source,
                });
            }
        }

        // Rebuild entry count from entries DB if meta is missing/corrupt.
        let count = match meta.get(&wtxn, META_COUNT_KEY) {
            Ok(Some(raw)) if raw.len() == 8 => u64::from_le_bytes(raw.try_into().unwrap_or([0; 8])),
            _ => {
                let mut n = 0u64;
                let iter = entries.iter(&wtxn).map_err(|source| LmdbBackendError::Io {
                    cache: cache_name.to_string(),
                    path: path.clone(),
                    source,
                })?;
                for item in iter {
                    item.map_err(|source| LmdbBackendError::Io {
                        cache: cache_name.to_string(),
                        path: path.clone(),
                        source,
                    })?;
                    n += 1;
                }
                meta.put(&mut wtxn, META_COUNT_KEY, &n.to_le_bytes())
                    .map_err(|source| LmdbBackendError::Io {
                        cache: cache_name.to_string(),
                        path: path.clone(),
                        source,
                    })?;
                n
            }
        };
        let _ = count;

        wtxn.commit().map_err(|source| LmdbBackendError::Io {
            cache: cache_name.to_string(),
            path: path.clone(),
            source,
        })?;

        Ok(Self {
            cache_name: cache_name.to_string(),
            path,
            env,
            meta,
            entries,
            max_entries: AtomicU64::new(max_entries),
            map_size_bytes: AtomicU64::new(map_size as u64),
            policy: RwLock::new(LmdbPolicy {
                when_full: lmdb.when_full,
                sample_size: lmdb.sample_size,
            }),
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn map_size_bytes(&self) -> u64 {
        self.map_size_bytes.load(Ordering::Relaxed)
    }

    pub fn apply_policy(&self, when_full: LmdbWhenFull, sample_size: u32) {
        *self.policy.write() = LmdbPolicy {
            when_full,
            sample_size,
        };
    }

    pub fn set_max_entries(&self, max_entries: u64) {
        self.max_entries.store(max_entries, Ordering::Relaxed);
    }

    pub fn max_entries(&self) -> u64 {
        self.max_entries.load(Ordering::Relaxed)
    }

    /// Grow map size in place when LMDB allows (Hot apply).
    pub fn grow_map_size(&self, new_size: u64) -> Result<(), LmdbBackendError> {
        let cur = self.map_size_bytes.load(Ordering::Relaxed);
        if new_size <= cur {
            return Ok(());
        }
        self.resize_to(align_map_size(new_size) as u64)
    }

    /// Shrink map size using the live ladder: in-place when used space fits, else
    /// evict until under the ceiling and retry, else clear all entries and retry.
    /// Caller must mark the instance rebuilding (Bypass) for the duration.
    pub fn shrink_map_size(&self, new_size: u64) -> Result<(), LmdbBackendError> {
        let cur = self.map_size_bytes.load(Ordering::Relaxed);
        let target = align_map_size(new_size) as u64;
        if target >= cur {
            return Ok(());
        }

        if self.used_bytes() <= target {
            return self.resize_to(target);
        }

        // Evict until page usage fits (or the store is empty).
        let mut guard = 0u32;
        while self.used_bytes() > target && guard < 1_000_000 {
            guard += 1;
            if !self.evict_one_arbitrary()? {
                break;
            }
        }
        if self.used_bytes() <= target {
            return self.resize_to(target);
        }

        tracing::warn!(
            cache = %self.cache_name,
            path = %self.path.display(),
            from = cur,
            to = target,
            "LMDB map_size shrink: clearing all entries to fit new ceiling"
        );
        self.clear_all_entries()?;
        if self.used_bytes() > target {
            return Err(LmdbBackendError::Io {
                cache: self.cache_name.clone(),
                path: self.path.clone(),
                source: heed::Error::Io(std::io::Error::other(format!(
                    "map_size shrink to {target} bytes failed: empty env still uses {} bytes \
                         (LMDB freelist/high-water). Change path to a new empty env or increase map_size.",
                    self.used_bytes()
                ))),
            });
        }
        self.resize_to(target)
    }

    fn resize_to(&self, aligned_bytes: u64) -> Result<(), LmdbBackendError> {
        // SAFETY: registry marks rebuilding / serializes map-size apply so no write
        // transactions are active across this call.
        unsafe {
            self.env
                .resize(aligned_bytes as usize)
                .map_err(|source| LmdbBackendError::Io {
                    cache: self.cache_name.clone(),
                    path: self.path.clone(),
                    source,
                })?;
        }
        self.map_size_bytes.store(aligned_bytes, Ordering::Relaxed);
        Ok(())
    }

    /// Delete every entry key (meta count reset). Used by the shrink nuclear step.
    pub fn clear_all_entries(&self) -> Result<(), LmdbBackendError> {
        let mut wtxn = self
            .env
            .write_txn()
            .map_err(|source| LmdbBackendError::Io {
                cache: self.cache_name.clone(),
                path: self.path.clone(),
                source,
            })?;
        let keys: Vec<Vec<u8>> = {
            let iter = self
                .entries
                .iter(&wtxn)
                .map_err(|source| LmdbBackendError::Io {
                    cache: self.cache_name.clone(),
                    path: self.path.clone(),
                    source,
                })?;
            let mut out = Vec::new();
            for item in iter {
                let (k, _) = item.map_err(|source| LmdbBackendError::Io {
                    cache: self.cache_name.clone(),
                    path: self.path.clone(),
                    source,
                })?;
                out.push(k.to_vec());
            }
            out
        };
        for key in &keys {
            self.entries
                .delete(&mut wtxn, key)
                .map_err(|source| LmdbBackendError::Io {
                    cache: self.cache_name.clone(),
                    path: self.path.clone(),
                    source,
                })?;
        }
        self.meta
            .put(&mut wtxn, META_COUNT_KEY, &0u64.to_le_bytes())
            .map_err(|source| LmdbBackendError::Io {
                cache: self.cache_name.clone(),
                path: self.path.clone(),
                source,
            })?;
        wtxn.commit().map_err(|source| LmdbBackendError::Io {
            cache: self.cache_name.clone(),
            path: self.path.clone(),
            source,
        })?;
        Ok(())
    }

    pub fn used_bytes(&self) -> u64 {
        let stat = self.env.stat();
        let pages = (stat.leaf_pages as u64)
            .saturating_add(stat.branch_pages as u64)
            .saturating_add(stat.overflow_pages as u64);
        pages.saturating_mul(stat.page_size as u64)
    }

    pub fn entry_count(&self) -> u64 {
        let rtxn = match self.env.read_txn() {
            Ok(t) => t,
            Err(e) => {
                tracing::warn!(
                    cache = %self.cache_name,
                    error = %e,
                    "LMDB read_txn failed for entry_count"
                );
                return 0;
            }
        };
        match self.meta.get(&rtxn, META_COUNT_KEY) {
            Ok(Some(raw)) if raw.len() == 8 => u64::from_le_bytes(raw.try_into().unwrap_or([0; 8])),
            Ok(_) => 0,
            Err(e) => {
                tracing::warn!(
                    cache = %self.cache_name,
                    error = %e,
                    "LMDB meta get failed for entry_count"
                );
                0
            }
        }
    }

    pub fn get(&self, key: &CacheKey, now: Instant) -> Option<CacheEntry> {
        match self.get_result(key, now) {
            CacheGetResult::Hit => self.get_fresh(key, now),
            _ => None,
        }
    }

    pub fn get_result(&self, key: &CacheKey, now: Instant) -> CacheGetResult {
        let rtxn = match self.env.read_txn() {
            Ok(t) => t,
            Err(e) => {
                tracing::error!(
                    cache = %self.cache_name,
                    error = %e,
                    "LMDB read_txn failed on get"
                );
                return CacheGetResult::Miss;
            }
        };
        let raw = match self.entries.get(&rtxn, key.0.as_slice()) {
            Ok(v) => v,
            Err(e) => {
                tracing::error!(
                    cache = %self.cache_name,
                    error = %e,
                    "LMDB get failed"
                );
                return CacheGetResult::Miss;
            }
        };
        let Some(raw) = raw else {
            return CacheGetResult::Miss;
        };
        let stored = match decode_value(raw) {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!(
                    cache = %self.cache_name,
                    error = %e,
                    "LMDB value decode failed; treating as miss"
                );
                return CacheGetResult::Miss;
            }
        };
        let now_unix = unix_now();
        if now_unix >= stored.expires_at_unix {
            drop(rtxn);
            let _ = self.remove(key);
            return CacheGetResult::Miss;
        }
        let _ = now; // Instant freshness applied in get_fresh
        CacheGetResult::Hit
    }

    fn get_fresh(&self, key: &CacheKey, now: Instant) -> Option<CacheEntry> {
        let rtxn = self.env.read_txn().ok()?;
        let raw = self.entries.get(&rtxn, key.0.as_slice()).ok()??;
        let stored = decode_value(raw).ok()?;
        let now_unix = unix_now();
        if now_unix >= stored.expires_at_unix {
            drop(rtxn);
            let _ = self.remove(key);
            return None;
        }
        Some(stored_to_entry(stored, now, now_unix))
    }

    /// Insert an entry. Returns `false` when refused under `when_full: refuse`.
    pub fn insert(&self, key: CacheKey, entry: CacheEntry, now: Instant) -> bool {
        match self.insert_inner(key, entry, now, true) {
            Ok(stored) => stored,
            Err(e) => {
                tracing::error!(
                    cache = %self.cache_name,
                    path = %self.path.display(),
                    error = %e,
                    "LMDB insert failed"
                );
                false
            }
        }
    }

    fn insert_inner(
        &self,
        key: CacheKey,
        entry: CacheEntry,
        now: Instant,
        allow_retry: bool,
    ) -> Result<bool, LmdbBackendError> {
        let max = self.max_entries.load(Ordering::Relaxed);
        let policy = *self.policy.read();
        let replacing = self.key_exists(&key)?;

        if max > 0 && !replacing {
            let count = self.entry_count();
            if count >= max {
                match policy.when_full {
                    LmdbWhenFull::Refuse => {
                        tracing::warn!(
                            cache = %self.cache_name,
                            max_entries = max,
                            entry_count = count,
                            "LMDB entry cap full; refusing fill (when_full=refuse)"
                        );
                        return Ok(false);
                    }
                    LmdbWhenFull::EvictOne | LmdbWhenFull::Sample => {
                        if !self.evict_victim(policy)? {
                            tracing::warn!(
                                cache = %self.cache_name,
                                "LMDB entry cap full; eviction found no victim"
                            );
                            return Ok(false);
                        }
                    }
                }
            }
        }

        let value = encode_entry(&entry, now);
        let mut wtxn = self
            .env
            .write_txn()
            .map_err(|source| LmdbBackendError::Io {
                cache: self.cache_name.clone(),
                path: self.path.clone(),
                source,
            })?;

        match self.entries.put(&mut wtxn, key.0.as_slice(), &value) {
            Ok(()) => {
                if !replacing {
                    self.bump_count(&mut wtxn, 1)?;
                }
                wtxn.commit().map_err(|source| LmdbBackendError::Io {
                    cache: self.cache_name.clone(),
                    path: self.path.clone(),
                    source,
                })?;
                Ok(true)
            }
            Err(heed::Error::Mdb(MdbError::MapFull)) => {
                drop(wtxn);
                tracing::warn!(
                    cache = %self.cache_name,
                    path = %self.path.display(),
                    "LMDB map full on put"
                );
                match policy.when_full {
                    LmdbWhenFull::Refuse => {
                        tracing::warn!(
                            cache = %self.cache_name,
                            "LMDB map full; refusing fill (when_full=refuse)"
                        );
                        Ok(false)
                    }
                    LmdbWhenFull::EvictOne | LmdbWhenFull::Sample if allow_retry => {
                        if !self.evict_victim(policy)? {
                            tracing::error!(
                                cache = %self.cache_name,
                                "LMDB map full; eviction found no victim"
                            );
                            return Ok(false);
                        }
                        self.insert_inner(key, entry, now, false)
                    }
                    _ => {
                        tracing::error!(
                            cache = %self.cache_name,
                            "LMDB map full; retry already attempted"
                        );
                        Ok(false)
                    }
                }
            }
            Err(source) => Err(LmdbBackendError::Io {
                cache: self.cache_name.clone(),
                path: self.path.clone(),
                source,
            }),
        }
    }

    pub fn remove(&self, key: &CacheKey) -> bool {
        let mut wtxn = match self.env.write_txn() {
            Ok(t) => t,
            Err(e) => {
                tracing::error!(cache = %self.cache_name, error = %e, "LMDB write_txn failed on remove");
                return false;
            }
        };
        let existed = match self.entries.get(&wtxn, key.0.as_slice()) {
            Ok(v) => v.is_some(),
            Err(e) => {
                tracing::error!(cache = %self.cache_name, error = %e, "LMDB get failed on remove");
                return false;
            }
        };
        if !existed {
            return false;
        }
        if let Err(e) = self.entries.delete(&mut wtxn, key.0.as_slice()) {
            tracing::error!(cache = %self.cache_name, error = %e, "LMDB delete failed");
            return false;
        }
        if let Err(e) = self.bump_count(&mut wtxn, -1) {
            tracing::error!(cache = %self.cache_name, error = %e, "LMDB count bump failed on remove");
            return false;
        }
        if let Err(e) = wtxn.commit() {
            tracing::error!(cache = %self.cache_name, error = %e, "LMDB commit failed on remove");
            return false;
        }
        true
    }

    fn key_exists(&self, key: &CacheKey) -> Result<bool, LmdbBackendError> {
        let rtxn = self.env.read_txn().map_err(|source| LmdbBackendError::Io {
            cache: self.cache_name.clone(),
            path: self.path.clone(),
            source,
        })?;
        let v = self
            .entries
            .get(&rtxn, key.0.as_slice())
            .map_err(|source| LmdbBackendError::Io {
                cache: self.cache_name.clone(),
                path: self.path.clone(),
                source,
            })?;
        Ok(v.is_some())
    }

    fn bump_count(&self, wtxn: &mut heed::RwTxn<'_>, delta: i64) -> Result<(), LmdbBackendError> {
        let cur = match self.meta.get(wtxn, META_COUNT_KEY) {
            Ok(Some(raw)) if raw.len() == 8 => u64::from_le_bytes(raw.try_into().unwrap_or([0; 8])),
            _ => 0,
        };
        let next = if delta >= 0 {
            cur.saturating_add(delta as u64)
        } else {
            cur.saturating_sub((-delta) as u64)
        };
        self.meta
            .put(wtxn, META_COUNT_KEY, &next.to_le_bytes())
            .map_err(|source| LmdbBackendError::Io {
                cache: self.cache_name.clone(),
                path: self.path.clone(),
                source,
            })?;
        Ok(())
    }

    fn evict_victim(&self, policy: LmdbPolicy) -> Result<bool, LmdbBackendError> {
        match policy.when_full {
            LmdbWhenFull::Refuse => Ok(false),
            LmdbWhenFull::EvictOne => self.evict_one_arbitrary(),
            LmdbWhenFull::Sample => self.evict_from_sample(policy.sample_size.max(1) as usize),
        }
    }

    fn evict_one_arbitrary(&self) -> Result<bool, LmdbBackendError> {
        let mut wtxn = self
            .env
            .write_txn()
            .map_err(|source| LmdbBackendError::Io {
                cache: self.cache_name.clone(),
                path: self.path.clone(),
                source,
            })?;
        let mut iter = self
            .entries
            .iter(&wtxn)
            .map_err(|source| LmdbBackendError::Io {
                cache: self.cache_name.clone(),
                path: self.path.clone(),
                source,
            })?;
        let Some(item) = iter.next() else {
            return Ok(false);
        };
        let (key, _val) = item.map_err(|source| LmdbBackendError::Io {
            cache: self.cache_name.clone(),
            path: self.path.clone(),
            source,
        })?;
        // Arbitrary cursor victim (first key); sample strategy prefers expiry.
        let victim_key = key.to_vec();
        drop(iter);
        self.entries
            .delete(&mut wtxn, &victim_key)
            .map_err(|source| LmdbBackendError::Io {
                cache: self.cache_name.clone(),
                path: self.path.clone(),
                source,
            })?;
        self.bump_count(&mut wtxn, -1)?;
        wtxn.commit().map_err(|source| LmdbBackendError::Io {
            cache: self.cache_name.clone(),
            path: self.path.clone(),
            source,
        })?;
        Ok(true)
    }

    fn evict_from_sample(&self, sample_size: usize) -> Result<bool, LmdbBackendError> {
        let rtxn = self.env.read_txn().map_err(|source| LmdbBackendError::Io {
            cache: self.cache_name.clone(),
            path: self.path.clone(),
            source,
        })?;
        let iter = self
            .entries
            .iter(&rtxn)
            .map_err(|source| LmdbBackendError::Io {
                cache: self.cache_name.clone(),
                path: self.path.clone(),
                source,
            })?;
        let now_unix = unix_now();
        let mut best_expired: Option<Vec<u8>> = None;
        let mut best_soon: Option<(u64, Vec<u8>)> = None;
        let mut seen = 0usize;
        for item in iter {
            let (key, val) = item.map_err(|source| LmdbBackendError::Io {
                cache: self.cache_name.clone(),
                path: self.path.clone(),
                source,
            })?;
            seen += 1;
            if let Ok(stored) = decode_value(val) {
                if now_unix >= stored.expires_at_unix {
                    best_expired = Some(key.to_vec());
                    break;
                }
                match &best_soon {
                    None => best_soon = Some((stored.expires_at_unix, key.to_vec())),
                    Some((exp, _)) if stored.expires_at_unix < *exp => {
                        best_soon = Some((stored.expires_at_unix, key.to_vec()));
                    }
                    _ => {}
                }
            }
            if seen >= sample_size {
                break;
            }
        }
        drop(rtxn);
        let victim = best_expired.or_else(|| best_soon.map(|(_, k)| k));
        let Some(victim) = victim else {
            return Ok(false);
        };
        let mut wtxn = self
            .env
            .write_txn()
            .map_err(|source| LmdbBackendError::Io {
                cache: self.cache_name.clone(),
                path: self.path.clone(),
                source,
            })?;
        self.entries
            .delete(&mut wtxn, &victim)
            .map_err(|source| LmdbBackendError::Io {
                cache: self.cache_name.clone(),
                path: self.path.clone(),
                source,
            })?;
        self.bump_count(&mut wtxn, -1)?;
        wtxn.commit().map_err(|source| LmdbBackendError::Io {
            cache: self.cache_name.clone(),
            path: self.path.clone(),
            source,
        })?;
        Ok(true)
    }
}

fn verify_format(path: &Path, raw: &[u8]) -> Result<(), LmdbBackendError> {
    if raw.len() < 8 || &raw[0..4] != FORMAT_MAGIC {
        return Err(LmdbBackendError::FormatMagic {
            path: path.to_path_buf(),
        });
    }
    let found = u32::from_le_bytes(raw[4..8].try_into().unwrap_or([0; 4]));
    if found != FORMAT_VERSION {
        return Err(LmdbBackendError::FormatMismatch {
            path: path.to_path_buf(),
            found,
            expected: FORMAT_VERSION,
        });
    }
    Ok(())
}

struct StoredEntry {
    filled_at_unix: u64,
    expires_at_unix: u64,
    kind: EntryKind,
    wire: Arc<[u8]>,
}

fn encode_entry(entry: &CacheEntry, now: Instant) -> Vec<u8> {
    let now_unix = unix_now();
    let remaining = entry.ttl_remaining_secs(now) as u64;
    let age = now
        .checked_duration_since(entry.filled_at)
        .unwrap_or(Duration::ZERO)
        .as_secs();
    let filled_at_unix = now_unix.saturating_sub(age);
    let expires_at_unix = now_unix.saturating_add(remaining);
    let mut buf = Vec::with_capacity(VALUE_HEADER_LEN + entry.wire.len());
    buf.extend_from_slice(&filled_at_unix.to_le_bytes());
    buf.extend_from_slice(&expires_at_unix.to_le_bytes());
    buf.push(kind_to_u8(entry.kind));
    let wire_len = entry.wire.len() as u32;
    buf.extend_from_slice(&wire_len.to_le_bytes());
    buf.extend_from_slice(&entry.wire);
    buf
}

fn decode_value(raw: &[u8]) -> Result<StoredEntry, String> {
    if raw.len() < VALUE_HEADER_LEN {
        return Err(format!("value too short ({})", raw.len()));
    }
    let filled_at_unix = u64::from_le_bytes(raw[0..8].try_into().unwrap());
    let expires_at_unix = u64::from_le_bytes(raw[8..16].try_into().unwrap());
    let kind = u8_to_kind(raw[16])?;
    let wire_len = u32::from_le_bytes(raw[17..21].try_into().unwrap()) as usize;
    if raw.len() < VALUE_HEADER_LEN + wire_len {
        return Err("wire truncated".into());
    }
    let wire: Arc<[u8]> = Arc::from(raw[VALUE_HEADER_LEN..VALUE_HEADER_LEN + wire_len].to_vec());
    Ok(StoredEntry {
        filled_at_unix,
        expires_at_unix,
        kind,
        wire,
    })
}

fn stored_to_entry(stored: StoredEntry, now: Instant, now_unix: u64) -> CacheEntry {
    let remaining = stored.expires_at_unix.saturating_sub(now_unix);
    let age = now_unix.saturating_sub(stored.filled_at_unix);
    CacheEntry {
        kind: stored.kind,
        wire: stored.wire,
        filled_at: now.checked_sub(Duration::from_secs(age)).unwrap_or(now),
        expires_at: now + Duration::from_secs(remaining),
    }
}

fn kind_to_u8(kind: EntryKind) -> u8 {
    match kind {
        EntryKind::Positive => 0,
        EntryKind::NxDomain => 1,
        EntryKind::NoData => 2,
        EntryKind::ServFail => 3,
    }
}

fn u8_to_kind(v: u8) -> Result<EntryKind, String> {
    match v {
        0 => Ok(EntryKind::Positive),
        1 => Ok(EntryKind::NxDomain),
        2 => Ok(EntryKind::NoData),
        3 => Ok(EntryKind::ServFail),
        other => Err(format!("unknown entry kind {other}")),
    }
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::ZERO)
        .as_secs()
}

fn align_map_size(bytes: u64) -> usize {
    // LMDB requires map size to be a multiple of the OS page size.
    let page = 4096usize;
    let n = (bytes as usize).max(page);
    n.div_ceil(page) * page
}

#[cfg(test)]
mod tests {
    use super::*;
    use conduit_config::lookup::{
        CacheBackendType, CompiledMemoryCache, CompiledNegativeCache, CompiledTruncatedUdp,
        EvictionMode, OnHitResponseRules,
    };
    use std::sync::Arc;

    fn sample_wire() -> Arc<[u8]> {
        Arc::from([0u8, 1, 2, 3, 4, 5, 6, 7] as [u8; 8])
    }

    fn test_cfg(path: PathBuf, max_entries: u64, when_full: LmdbWhenFull) -> CompiledCacheInstance {
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
            lmdb: Some(CompiledLmdbCache {
                path,
                map_size_bytes: 2 * 1024 * 1024,
                when_full,
                sample_size: 16,
            }),
            max_entries,
        }
    }

    fn entry(now: Instant, ttl: u32) -> CacheEntry {
        CacheEntry {
            kind: EntryKind::Positive,
            wire: sample_wire(),
            filled_at: now,
            expires_at: now + Duration::from_secs(ttl as u64),
        }
    }

    #[test]
    fn open_creates_missing_parent_directories() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a").join("b").join("env");
        assert!(!path.exists());
        let cfg = test_cfg(path.clone(), 0, LmdbWhenFull::EvictOne);
        let backend = LmdbCacheBackend::open(&cfg).unwrap();
        assert!(path.is_dir());
        let now = Instant::now();
        assert!(backend.insert(CacheKey(b"k".to_vec()), entry(now, 60), now));
        drop(backend);
        assert!(path.join("data.mdb").exists() || path.exists());
    }

    #[test]
    fn round_trip_and_persist() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("env");
        let cfg = test_cfg(path.clone(), 0, LmdbWhenFull::EvictOne);
        let backend = LmdbCacheBackend::open(&cfg).unwrap();
        let now = Instant::now();
        let key = CacheKey(b"k1".to_vec());
        assert!(backend.insert(key.clone(), entry(now, 120), now));
        assert_eq!(backend.get_result(&key, now), CacheGetResult::Hit);
        assert_eq!(backend.entry_count(), 1);
        drop(backend);

        let backend2 = LmdbCacheBackend::open(&cfg).unwrap();
        assert_eq!(
            backend2.get_result(&key, Instant::now()),
            CacheGetResult::Hit
        );
        assert_eq!(backend2.entry_count(), 1);
    }

    #[test]
    fn lazy_expiry_is_miss() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = test_cfg(dir.path().join("env"), 0, LmdbWhenFull::EvictOne);
        let backend = LmdbCacheBackend::open(&cfg).unwrap();
        let key = CacheKey(b"stale".to_vec());
        // Write a value with expires_at_unix in the past directly.
        let mut wtxn = backend.env.write_txn().unwrap();
        let mut buf = Vec::new();
        let past = unix_now().saturating_sub(10);
        buf.extend_from_slice(&past.to_le_bytes());
        buf.extend_from_slice(&past.to_le_bytes());
        buf.push(0);
        let wire = sample_wire();
        buf.extend_from_slice(&(wire.len() as u32).to_le_bytes());
        buf.extend_from_slice(&wire);
        backend
            .entries
            .put(&mut wtxn, key.0.as_slice(), &buf)
            .unwrap();
        backend
            .meta
            .put(&mut wtxn, META_COUNT_KEY, &1u64.to_le_bytes())
            .unwrap();
        wtxn.commit().unwrap();

        assert_eq!(
            backend.get_result(&key, Instant::now()),
            CacheGetResult::Miss
        );
    }

    #[test]
    fn refuse_at_entry_cap() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = test_cfg(dir.path().join("env"), 1, LmdbWhenFull::Refuse);
        let backend = LmdbCacheBackend::open(&cfg).unwrap();
        let now = Instant::now();
        assert!(backend.insert(CacheKey(b"a".to_vec()), entry(now, 60), now));
        assert!(!backend.insert(CacheKey(b"b".to_vec()), entry(now, 60), now));
        assert_eq!(backend.entry_count(), 1);
    }

    #[test]
    fn evict_one_at_entry_cap() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = test_cfg(dir.path().join("env"), 1, LmdbWhenFull::EvictOne);
        let backend = LmdbCacheBackend::open(&cfg).unwrap();
        let now = Instant::now();
        assert!(backend.insert(CacheKey(b"a".to_vec()), entry(now, 60), now));
        assert!(backend.insert(CacheKey(b"b".to_vec()), entry(now, 60), now));
        assert_eq!(backend.entry_count(), 1);
        assert_eq!(
            backend.get_result(&CacheKey(b"b".to_vec()), now),
            CacheGetResult::Hit
        );
    }

    #[test]
    fn lazy_expiry_after_wall_clock_sleep() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = test_cfg(dir.path().join("env"), 0, LmdbWhenFull::EvictOne);
        let backend = LmdbCacheBackend::open(&cfg).unwrap();
        let now = Instant::now();
        let key = CacheKey(b"short-ttl".to_vec());
        assert!(backend.insert(key.clone(), entry(now, 1), now));
        assert_eq!(
            backend.get_result(&key, Instant::now()),
            CacheGetResult::Hit
        );
        // Inspect stored unix expiry is ~1s ahead.
        {
            let rtxn = backend.env.read_txn().unwrap();
            let raw = backend
                .entries
                .get(&rtxn, key.0.as_slice())
                .unwrap()
                .unwrap();
            let stored = decode_value(raw).unwrap();
            let now_u = unix_now();
            let remaining = stored.expires_at_unix.saturating_sub(now_u);
            assert!(
                remaining <= 2,
                "expected ~1s remaining at insert+immediate read, got {remaining} (expires={}, now={})",
                stored.expires_at_unix,
                now_u
            );
        }
        std::thread::sleep(Duration::from_millis(1500));
        assert_eq!(
            backend.get_result(&key, Instant::now()),
            CacheGetResult::Miss,
            "entry should miss after wall-clock TTL expiry"
        );
    }

    #[test]
    fn format_mismatch_fails() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("env");
        let cfg = test_cfg(path.clone(), 0, LmdbWhenFull::EvictOne);
        let backend = LmdbCacheBackend::open(&cfg).unwrap();
        {
            let mut wtxn = backend.env.write_txn().unwrap();
            let mut buf = Vec::new();
            buf.extend_from_slice(FORMAT_MAGIC);
            buf.extend_from_slice(&99u32.to_le_bytes());
            backend.meta.put(&mut wtxn, META_FORMAT_KEY, &buf).unwrap();
            wtxn.commit().unwrap();
        }
        drop(backend);
        match LmdbCacheBackend::open(&cfg) {
            Ok(_) => panic!("expected format mismatch"),
            Err(err) => {
                let msg = err.to_string();
                assert!(
                    msg.contains("format mismatch") || msg.contains("Move or delete"),
                    "{msg}"
                );
            }
        }
    }
}
