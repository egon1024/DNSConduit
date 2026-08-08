//! LMDB durable answer-cache backend (`heed`, safe sync default).
//!
//! Hash-sharded across N independent environments under one operator `path`
//! (flat `NO_SUB_DIR` files + Conduit sidecar). Legacy single-directory envs
//! (`data.mdb` / `lock.mdb`) open as N=1 when no sidecar is present.

use super::entry::{CacheEntry, EntryKind};
use super::key::CacheKey;
use super::memory::CacheGetResult;
use conduit_config::lookup::{
    CompiledCacheInstance, CompiledLmdbCache, LmdbWhenFull, MAX_LMDB_SHARD_COUNT,
};
use heed::types::Bytes;
use heed::{Database, Env, EnvFlags, EnvOpenOptions, MdbError};
use parking_lot::RwLock;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// On-disk format magic (`CDCL` = Conduit Cache LMDB).
const FORMAT_MAGIC: &[u8; 4] = b"CDCL";
/// Bump when the on-disk value layout changes; mismatched envs fail closed.
pub const FORMAT_VERSION: u32 = 1;

/// Sidecar layout version for multi-env sharding metadata.
const SHARD_LAYOUT_VERSION: u32 = 1;
const SIDECAR_FILE: &str = "conduit-lmdb-shards.json";

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
    #[error("LMDB shard layout error at '{path}': {message}")]
    Layout { path: PathBuf, message: String },
    #[error("LMDB I/O error for cache '{cache}' at '{path}': {source}")]
    Io {
        cache: String,
        path: PathBuf,
        source: heed::Error,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LayoutKind {
    /// Pre-sharding directory env (`data.mdb` / `lock.mdb` under `path`).
    Legacy,
    /// Flat `NO_SUB_DIR` files `shard-{i}` + sidecar.
    Sharded,
}

struct LmdbShard {
    /// Env file path (may be rewritten after a Warm shard_count reopen relocate).
    env_path: RwLock<PathBuf>,
    env: Env,
    meta: Database<Bytes, Bytes>,
    entries: Database<Bytes, Bytes>,
    max_entries: AtomicU64,
    map_size_bytes: AtomicU64,
}

pub struct LmdbCacheBackend {
    cache_name: String,
    /// Operator cache directory (rewritten after Warm shard_count reopen relocate).
    path: RwLock<PathBuf>,
    layout: LayoutKind,
    shards: Vec<LmdbShard>,
    total_map_size_bytes: AtomicU64,
    total_max_entries: AtomicU64,
    policy: RwLock<LmdbPolicy>,
}

#[derive(Debug, Clone, Copy)]
struct LmdbPolicy {
    when_full: LmdbWhenFull,
    sample_size: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum OnDiskLayout {
    Empty,
    Legacy,
    Sharded { n: u32 },
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

        // Stale staging dirs / reopen artifacts from a crashed apply are not a live layout.
        remove_stale_reopen_staging(&path);

        let mut on_disk = detect_on_disk_layout(&path)?;
        if explicit_requires_abandon(lmdb.shard_count, on_disk) {
            tracing::info!(
                cache = %cache_name,
                path = %path.display(),
                explicit = ?lmdb.shard_count,
                on_disk = ?on_disk,
                "abandoning on-disk LMDB layout for explicit shard_count change"
            );
            remove_owned_layout_files(&path)?;
            on_disk = OnDiskLayout::Empty;
        }

        let effective_n =
            resolve_effective_shard_count(lmdb.shard_count, on_disk, lmdb.lookup_thread_count)?;

        let (layout, shards) = if on_disk == OnDiskLayout::Legacy && effective_n == 1 {
            let map_sizes = per_shard_map_sizes(lmdb.map_size_bytes, 1);
            let caps = per_shard_max_entries(max_entries, 1);
            let shard = open_directory_env(cache_name, &path, map_sizes[0], caps[0])?;
            (LayoutKind::Legacy, vec![shard])
        } else {
            let map_sizes = per_shard_map_sizes(lmdb.map_size_bytes, effective_n as usize);
            let caps = per_shard_max_entries(max_entries, effective_n as usize);
            let mut shards = Vec::with_capacity(effective_n as usize);
            for i in 0..effective_n as usize {
                let env_path = shard_file_path(&path, i);
                shards.push(open_nosubdir_env(
                    cache_name,
                    &env_path,
                    map_sizes[i],
                    caps[i],
                )?);
            }
            write_sidecar(&path, effective_n)?;
            (LayoutKind::Sharded, shards)
        };

        let aligned_total: u64 = shards
            .iter()
            .map(|s| s.map_size_bytes.load(Ordering::Relaxed))
            .sum();

        Ok(Self {
            cache_name: cache_name.to_string(),
            path: RwLock::new(path),
            layout,
            shards,
            total_map_size_bytes: AtomicU64::new(aligned_total),
            total_max_entries: AtomicU64::new(max_entries),
            policy: RwLock::new(LmdbPolicy {
                when_full: lmdb.when_full,
                sample_size: lmdb.sample_size,
            }),
        })
    }

    /// Open a replacement shard layout in a sibling staging directory for Warm reopen.
    ///
    /// Caller must Arc-swap this backend into service (dropping the prior env), then call
    /// [`finalize_shard_reopen`] so abandoned files under the operator `path` are removed and
    /// the staging directory is renamed into place.
    pub fn open_for_shard_reopen(
        cfg: &CompiledCacheInstance,
    ) -> Result<(Self, PathBuf, PathBuf), LmdbBackendError> {
        let lmdb = cfg.lmdb.as_ref().ok_or_else(|| LmdbBackendError::Io {
            cache: cfg.name.clone(),
            path: PathBuf::new(),
            source: heed::Error::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "missing lmdb compiled config",
            )),
        })?;
        let operator = lmdb.path.clone();
        let staging = shard_reopen_staging_dir(&operator);
        if staging.exists() {
            std::fs::remove_dir_all(&staging).map_err(|e| LmdbBackendError::Layout {
                path: staging.clone(),
                message: format!("failed to clear shard reopen staging dir: {e}"),
            })?;
        }
        std::fs::create_dir_all(&staging).map_err(|source| LmdbBackendError::CreateDir {
            path: staging.clone(),
            source,
        })?;

        let mut staging_lmdb = lmdb.clone();
        staging_lmdb.path = staging.clone();
        // Staging is empty — open with explicit shard_count (required for abandon reopen).
        let backend = Self::open_at(&cfg.name, cfg.max_entries, &staging_lmdb)?;
        Ok((backend, operator, staging))
    }

    /// After the prior backend has been dropped, move staging into the operator path and
    /// rewrite this backend's path strings. Does not close the live LMDB envs (fds follow rename).
    pub fn finalize_shard_reopen(
        &self,
        operator: &Path,
        staging: &Path,
    ) -> Result<(), LmdbBackendError> {
        let abandoned = shard_reopen_abandoned_dir(operator);
        if abandoned.exists() {
            let _ = std::fs::remove_dir_all(&abandoned);
        }
        if operator.exists() {
            std::fs::rename(operator, &abandoned).map_err(|e| LmdbBackendError::Layout {
                path: operator.to_path_buf(),
                message: format!(
                    "failed to move abandoned LMDB layout aside before shard reopen finalize: {e}"
                ),
            })?;
        }
        std::fs::rename(staging, operator).map_err(|e| LmdbBackendError::Layout {
            path: staging.to_path_buf(),
            message: format!("failed to install new LMDB shard layout at operator path: {e}"),
        })?;
        if let Err(e) = std::fs::remove_dir_all(&abandoned) {
            tracing::warn!(
                cache = %self.cache_name,
                path = %abandoned.display(),
                error = %e,
                "failed to delete abandoned LMDB layout after successful shard_count reopen"
            );
        }
        self.rewrite_paths_after_reopen(staging, operator);
        Ok(())
    }

    fn rewrite_paths_after_reopen(&self, staging: &Path, operator: &Path) {
        {
            let mut path = self.path.write();
            if path.as_os_str() == staging.as_os_str() {
                *path = operator.to_path_buf();
            }
        }
        for shard in &self.shards {
            let mut env_path = shard.env_path.write();
            if let Ok(rel) = env_path.strip_prefix(staging) {
                *env_path = operator.join(rel);
            } else if let Some(rest) = env_path
                .to_str()
                .and_then(|s| s.strip_prefix(&*staging.to_string_lossy()))
            {
                let rest = rest.trim_start_matches(std::path::MAIN_SEPARATOR);
                *env_path = operator.join(rest);
            }
        }
    }

    pub fn path(&self) -> PathBuf {
        self.path.read().clone()
    }

    pub fn shard_count(&self) -> usize {
        self.shards.len()
    }

    pub fn layout_is_legacy(&self) -> bool {
        self.layout == LayoutKind::Legacy
    }

    pub fn map_size_bytes(&self) -> u64 {
        self.total_map_size_bytes.load(Ordering::Relaxed)
    }

    pub fn apply_policy(&self, when_full: LmdbWhenFull, sample_size: u32) {
        *self.policy.write() = LmdbPolicy {
            when_full,
            sample_size,
        };
    }

    pub fn set_max_entries(&self, max_entries: u64) {
        self.total_max_entries.store(max_entries, Ordering::Relaxed);
        let caps = per_shard_max_entries(max_entries, self.shards.len());
        for (shard, cap) in self.shards.iter().zip(caps) {
            shard.max_entries.store(cap, Ordering::Relaxed);
        }
    }

    pub fn max_entries(&self) -> u64 {
        self.total_max_entries.load(Ordering::Relaxed)
    }

    /// Grow map size in place when LMDB allows (Hot apply). `new_size` is the total budget.
    pub fn grow_map_size(&self, new_size: u64) -> Result<(), LmdbBackendError> {
        let cur = self.total_map_size_bytes.load(Ordering::Relaxed);
        if new_size <= cur {
            return Ok(());
        }
        self.resize_total_to(align_map_size(new_size) as u64)
    }

    /// Shrink total map size using the live ladder across shards.
    pub fn shrink_map_size(&self, new_size: u64) -> Result<(), LmdbBackendError> {
        let cur = self.total_map_size_bytes.load(Ordering::Relaxed);
        let target = align_map_size(new_size) as u64;
        if target >= cur {
            return Ok(());
        }

        if self.used_bytes() <= target {
            return self.resize_total_to(target);
        }

        let mut guard = 0u32;
        while self.used_bytes() > target && guard < 1_000_000 {
            guard += 1;
            if !self.evict_one_arbitrary_any_shard()? {
                break;
            }
        }
        if self.used_bytes() <= target {
            return self.resize_total_to(target);
        }

        tracing::warn!(
            cache = %self.cache_name,
            path = %self.path.read().display(),
            from = cur,
            to = target,
            "LMDB map_size shrink: clearing all entries to fit new ceiling"
        );
        self.clear_all_entries()?;
        if self.used_bytes() > target {
            return Err(LmdbBackendError::Io {
                cache: self.cache_name.clone(),
                path: self.path(),
                source: heed::Error::Io(std::io::Error::other(format!(
                    "map_size shrink to {target} bytes failed: empty env still uses {} bytes \
                         (LMDB freelist/high-water). Change path to a new empty env or increase map_size.",
                    self.used_bytes()
                ))),
            });
        }
        self.resize_total_to(target)
    }

    fn resize_total_to(&self, aligned_total: u64) -> Result<(), LmdbBackendError> {
        let sizes = per_shard_map_sizes(aligned_total, self.shards.len());
        for (shard, size) in self.shards.iter().zip(sizes) {
            // SAFETY: registry marks rebuilding / serializes map-size apply so no write
            // transactions are active across this call.
            unsafe {
                shard
                    .env
                    .resize(size)
                    .map_err(|source| LmdbBackendError::Io {
                        cache: self.cache_name.clone(),
                        path: shard_env_path(shard),
                        source,
                    })?;
            }
            shard.map_size_bytes.store(size as u64, Ordering::Relaxed);
        }
        let sum: u64 = self
            .shards
            .iter()
            .map(|s| s.map_size_bytes.load(Ordering::Relaxed))
            .sum();
        self.total_map_size_bytes.store(sum, Ordering::Relaxed);
        Ok(())
    }

    /// Delete every entry key (meta count reset). Used by the shrink nuclear step.
    pub fn clear_all_entries(&self) -> Result<(), LmdbBackendError> {
        for shard in &self.shards {
            clear_shard_entries(shard, &self.cache_name)?;
        }
        Ok(())
    }

    pub fn used_bytes(&self) -> u64 {
        self.shards.iter().map(shard_used_bytes).sum()
    }

    pub fn entry_count(&self) -> u64 {
        self.shards
            .iter()
            .map(|s| shard_entry_count(s, &self.cache_name))
            .sum()
    }

    pub fn get(&self, key: &CacheKey, now: Instant) -> Option<CacheEntry> {
        match self.get_result(key, now) {
            CacheGetResult::Hit => self.get_fresh(key, now),
            _ => None,
        }
    }

    pub fn get_result(&self, key: &CacheKey, now: Instant) -> CacheGetResult {
        let shard = self.shard_for_key(key);
        let rtxn = match shard.env.read_txn() {
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
        let raw = match shard.entries.get(&rtxn, key.0.as_slice()) {
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
        let _ = now;
        CacheGetResult::Hit
    }

    fn get_fresh(&self, key: &CacheKey, now: Instant) -> Option<CacheEntry> {
        let shard = self.shard_for_key(key);
        let rtxn = shard.env.read_txn().ok()?;
        let raw = shard.entries.get(&rtxn, key.0.as_slice()).ok()??;
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
                    path = %self.path.read().display(),
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
        let shard = self.shard_for_key(&key);
        let max = shard.max_entries.load(Ordering::Relaxed);
        let policy = *self.policy.read();
        let replacing = shard_key_exists(shard, &self.cache_name, &key)?;

        if max > 0 && !replacing {
            let count = shard_entry_count(shard, &self.cache_name);
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
                        if !evict_victim(shard, &self.cache_name, policy)? {
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
        let mut wtxn = shard
            .env
            .write_txn()
            .map_err(|source| LmdbBackendError::Io {
                cache: self.cache_name.clone(),
                path: shard_env_path(shard),
                source,
            })?;

        match shard.entries.put(&mut wtxn, key.0.as_slice(), &value) {
            Ok(()) => {
                if !replacing {
                    bump_count(shard, &self.cache_name, &mut wtxn, 1)?;
                }
                wtxn.commit().map_err(|source| LmdbBackendError::Io {
                    cache: self.cache_name.clone(),
                    path: shard_env_path(shard),
                    source,
                })?;
                Ok(true)
            }
            Err(heed::Error::Mdb(MdbError::MapFull)) => {
                drop(wtxn);
                tracing::warn!(
                    cache = %self.cache_name,
                    path = %shard_env_path(shard).display(),
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
                        if !evict_victim(shard, &self.cache_name, policy)? {
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
                path: shard_env_path(shard),
                source,
            }),
        }
    }

    pub fn remove(&self, key: &CacheKey) -> bool {
        let shard = self.shard_for_key(key);
        let mut wtxn = match shard.env.write_txn() {
            Ok(t) => t,
            Err(e) => {
                tracing::error!(cache = %self.cache_name, error = %e, "LMDB write_txn failed on remove");
                return false;
            }
        };
        let existed = match shard.entries.get(&wtxn, key.0.as_slice()) {
            Ok(v) => v.is_some(),
            Err(e) => {
                tracing::error!(cache = %self.cache_name, error = %e, "LMDB get failed on remove");
                return false;
            }
        };
        if !existed {
            return false;
        }
        if let Err(e) = shard.entries.delete(&mut wtxn, key.0.as_slice()) {
            tracing::error!(cache = %self.cache_name, error = %e, "LMDB delete failed");
            return false;
        }
        if let Err(e) = bump_count(shard, &self.cache_name, &mut wtxn, -1) {
            tracing::error!(cache = %self.cache_name, error = %e, "LMDB count bump failed on remove");
            return false;
        }
        if let Err(e) = wtxn.commit() {
            tracing::error!(cache = %self.cache_name, error = %e, "LMDB commit failed on remove");
            return false;
        }
        true
    }

    fn shard_for_key(&self, key: &CacheKey) -> &LmdbShard {
        &self.shards[shard_index_for_key(key, self.shards.len())]
    }

    fn evict_one_arbitrary_any_shard(&self) -> Result<bool, LmdbBackendError> {
        for shard in &self.shards {
            if evict_one_arbitrary(shard, &self.cache_name)? {
                return Ok(true);
            }
        }
        Ok(false)
    }
}

fn shard_index_for_key(key: &CacheKey, n: usize) -> usize {
    use std::hash::{Hash, Hasher};
    debug_assert!(n > 0);
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    key.0.hash(&mut hasher);
    (hasher.finish() as usize) % n
}

/// Resolve effective shard count at open (design D3).
pub(crate) fn resolve_effective_shard_count(
    explicit: Option<u32>,
    on_disk: OnDiskLayout,
    lookup_thread_count: u32,
) -> Result<u32, LmdbBackendError> {
    if let Some(n) = explicit {
        return Ok(n.clamp(1, MAX_LMDB_SHARD_COUNT));
    }
    match on_disk {
        OnDiskLayout::Sharded { n } => Ok(n.clamp(1, MAX_LMDB_SHARD_COUNT)),
        OnDiskLayout::Legacy => Ok(1),
        OnDiskLayout::Empty => {
            let threads = lookup_thread_count.max(1);
            let n = threads.saturating_mul(2).clamp(1, MAX_LMDB_SHARD_COUNT);
            Ok(n)
        }
    }
}

fn on_disk_shard_count(on_disk: OnDiskLayout) -> Option<u32> {
    match on_disk {
        OnDiskLayout::Empty => None,
        OnDiskLayout::Legacy => Some(1),
        OnDiskLayout::Sharded { n } => Some(n),
    }
}

/// True when an explicit `shard_count` differs from a recognizable on-disk layout N.
pub(crate) fn explicit_requires_abandon(explicit: Option<u32>, on_disk: OnDiskLayout) -> bool {
    let Some(n) = explicit else {
        return false;
    };
    let want = n.clamp(1, MAX_LMDB_SHARD_COUNT);
    match on_disk_shard_count(on_disk) {
        Some(disk) => disk != want,
        None => false,
    }
}

fn shard_reopen_staging_dir(operator: &Path) -> PathBuf {
    let mut s = operator.as_os_str().to_owned();
    s.push(".conduit-lmdb-reopen");
    PathBuf::from(s)
}

fn shard_reopen_abandoned_dir(operator: &Path) -> PathBuf {
    let mut s = operator.as_os_str().to_owned();
    s.push(".conduit-lmdb-abandoned");
    PathBuf::from(s)
}

fn remove_stale_reopen_staging(operator: &Path) {
    let staging = shard_reopen_staging_dir(operator);
    if staging.exists() {
        if let Err(e) = std::fs::remove_dir_all(&staging) {
            tracing::warn!(
                path = %staging.display(),
                error = %e,
                "failed to remove stale LMDB shard reopen staging directory"
            );
        }
    }
    let abandoned = shard_reopen_abandoned_dir(operator);
    if abandoned.exists() {
        if let Err(e) = std::fs::remove_dir_all(&abandoned) {
            tracing::warn!(
                path = %abandoned.display(),
                error = %e,
                "failed to remove leftover abandoned LMDB layout directory"
            );
        }
    }
}

/// Remove Conduit-owned LMDB layout files under `path` (legacy env and/or shards + sidecar).
fn remove_owned_layout_files(path: &Path) -> Result<(), LmdbBackendError> {
    let sidecar = sidecar_path(path);
    if sidecar.exists() {
        std::fs::remove_file(&sidecar).map_err(|e| LmdbBackendError::Layout {
            path: path.to_path_buf(),
            message: format!("failed to remove shard sidecar during abandon: {e}"),
        })?;
    }
    if let Ok(entries) = std::fs::read_dir(path) {
        for ent in entries.flatten() {
            let name = ent.file_name();
            let name = name.to_string_lossy();
            let remove = name == "data.mdb"
                || name == "lock.mdb"
                || name.starts_with("shard-")
                || name.starts_with(SIDECAR_FILE);
            if remove {
                let p = ent.path();
                if p.is_dir() {
                    let _ = std::fs::remove_dir_all(&p);
                } else if let Err(e) = std::fs::remove_file(&p) {
                    return Err(LmdbBackendError::Layout {
                        path: path.to_path_buf(),
                        message: format!("failed to remove layout file '{}': {e}", p.display()),
                    });
                }
            }
        }
    }
    Ok(())
}

/// Per-shard map ceilings that sum to the aligned total budget.
pub(crate) fn per_shard_map_sizes(total_bytes: u64, n: usize) -> Vec<usize> {
    debug_assert!(n > 0);
    let total = align_map_size(total_bytes);
    let page = 4096usize;
    let base = (total / n) / page * page;
    let base = base.max(page);
    let mut sizes = vec![base; n];
    let sum: usize = sizes.iter().sum();
    if sum < total {
        sizes[0] += total - sum;
    } else if sum > total {
        // Extremely small totals: keep at least one page per shard when possible.
        let overflow = sum - total;
        if sizes[0] > page + overflow {
            sizes[0] -= overflow;
        }
    }
    sizes
}

/// Per-shard entry caps that sum to `max_entries` (`0` = unlimited on each shard).
pub(crate) fn per_shard_max_entries(max_entries: u64, n: usize) -> Vec<u64> {
    debug_assert!(n > 0);
    if max_entries == 0 {
        return vec![0; n];
    }
    let base = max_entries / n as u64;
    let rem = max_entries % n as u64;
    (0..n)
        .map(|i| if (i as u64) < rem { base + 1 } else { base })
        .collect()
}

fn shard_file_path(base: &Path, index: usize) -> PathBuf {
    base.join(format!("shard-{index}"))
}

fn sidecar_path(base: &Path) -> PathBuf {
    base.join(SIDECAR_FILE)
}

fn detect_on_disk_layout(path: &Path) -> Result<OnDiskLayout, LmdbBackendError> {
    let sidecar = sidecar_path(path);
    if sidecar.exists() {
        let raw = std::fs::read_to_string(&sidecar).map_err(|e| LmdbBackendError::Layout {
            path: path.to_path_buf(),
            message: format!("failed to read shard sidecar: {e}"),
        })?;
        let v: serde_json::Value =
            serde_json::from_str(&raw).map_err(|e| LmdbBackendError::Layout {
                path: path.to_path_buf(),
                message: format!("invalid shard sidecar JSON: {e}"),
            })?;
        let version =
            v.get("version")
                .and_then(|x| x.as_u64())
                .ok_or_else(|| LmdbBackendError::Layout {
                    path: path.to_path_buf(),
                    message: "shard sidecar missing version".into(),
                })?;
        if version != SHARD_LAYOUT_VERSION as u64 {
            return Err(LmdbBackendError::Layout {
                path: path.to_path_buf(),
                message: format!(
                    "unsupported shard layout version {version} (expected {SHARD_LAYOUT_VERSION})"
                ),
            });
        }
        let n = v
            .get("shard_count")
            .and_then(|x| x.as_u64())
            .ok_or_else(|| LmdbBackendError::Layout {
                path: path.to_path_buf(),
                message: "shard sidecar missing shard_count".into(),
            })?;
        if n == 0 || n > MAX_LMDB_SHARD_COUNT as u64 {
            return Err(LmdbBackendError::Layout {
                path: path.to_path_buf(),
                message: format!("shard sidecar shard_count {n} out of range"),
            });
        }
        // Fail closed if any shard file is missing.
        for i in 0..n as usize {
            let p = shard_file_path(path, i);
            if !p.exists() {
                return Err(LmdbBackendError::Layout {
                    path: path.to_path_buf(),
                    message: format!(
                        "shard sidecar claims N={n} but missing file {}",
                        p.display()
                    ),
                });
            }
        }
        return Ok(OnDiskLayout::Sharded { n: n as u32 });
    }

    let data = path.join("data.mdb");
    let lock = path.join("lock.mdb");
    if data.exists() || lock.exists() {
        return Ok(OnDiskLayout::Legacy);
    }

    // Any leftover shard-* without sidecar is corrupt/partial — fail closed.
    if let Ok(entries) = std::fs::read_dir(path) {
        for ent in entries.flatten() {
            let name = ent.file_name();
            let name = name.to_string_lossy();
            if name.starts_with("shard-") {
                return Err(LmdbBackendError::Layout {
                    path: path.to_path_buf(),
                    message: format!(
                        "found shard file '{name}' without sidecar; repair or recreate the cache path"
                    ),
                });
            }
        }
    }

    Ok(OnDiskLayout::Empty)
}

fn write_sidecar(path: &Path, n: u32) -> Result<(), LmdbBackendError> {
    // Extra string fields are ignored on read; they exist so operators opening the
    // file see that Conduit owns this metadata.
    let body = serde_json::json!({
        "version": SHARD_LAYOUT_VERSION,
        "shard_count": n,
        "generated_by": "dnsconduit",
        "do_not_edit": "Generated and owned by DNSConduit. Do not edit by hand; change lmdb.shard_count in config instead.",
    });
    let tmp = path.join(format!("{SIDECAR_FILE}.tmp"));
    std::fs::write(&tmp, body.to_string()).map_err(|e| LmdbBackendError::Layout {
        path: path.to_path_buf(),
        message: format!("failed to write shard sidecar temp: {e}"),
    })?;
    std::fs::rename(&tmp, sidecar_path(path)).map_err(|e| LmdbBackendError::Layout {
        path: path.to_path_buf(),
        message: format!("failed to install shard sidecar: {e}"),
    })?;
    Ok(())
}

fn open_directory_env(
    cache_name: &str,
    path: &Path,
    map_size: usize,
    max_entries: u64,
) -> Result<LmdbShard, LmdbBackendError> {
    // SAFETY: path is operator-controlled; registry holds one backend Arc per name.
    let env = unsafe {
        EnvOpenOptions::new()
            .map_size(map_size)
            .max_dbs(2)
            .open(path)
    }
    .map_err(|source| LmdbBackendError::OpenEnv {
        path: path.to_path_buf(),
        source,
    })?;
    finish_open_env(cache_name, path.to_path_buf(), env, map_size, max_entries)
}

fn open_nosubdir_env(
    cache_name: &str,
    env_path: &Path,
    map_size: usize,
    max_entries: u64,
) -> Result<LmdbShard, LmdbBackendError> {
    // SAFETY: path is operator-controlled; NO_SUB_DIR opens a file path (not a directory).
    let env = unsafe {
        let mut opts = EnvOpenOptions::new();
        opts.map_size(map_size).max_dbs(2);
        opts.flags(EnvFlags::NO_SUB_DIR);
        opts.open(env_path)
    }
    .map_err(|source| LmdbBackendError::OpenEnv {
        path: env_path.to_path_buf(),
        source,
    })?;
    finish_open_env(
        cache_name,
        env_path.to_path_buf(),
        env,
        map_size,
        max_entries,
    )
}

fn finish_open_env(
    cache_name: &str,
    env_path: PathBuf,
    env: Env,
    map_size: usize,
    max_entries: u64,
) -> Result<LmdbShard, LmdbBackendError> {
    let mut wtxn = env.write_txn().map_err(|source| LmdbBackendError::Io {
        cache: cache_name.to_string(),
        path: env_path.clone(),
        source,
    })?;
    let meta: Database<Bytes, Bytes> =
        env.create_database(&mut wtxn, Some(META_DB))
            .map_err(|source| LmdbBackendError::Io {
                cache: cache_name.to_string(),
                path: env_path.clone(),
                source,
            })?;
    let entries: Database<Bytes, Bytes> = env
        .create_database(&mut wtxn, Some(ENTRIES_DB))
        .map_err(|source| LmdbBackendError::Io {
            cache: cache_name.to_string(),
            path: env_path.clone(),
            source,
        })?;

    match meta.get(&wtxn, META_FORMAT_KEY) {
        Ok(Some(raw)) => {
            verify_format(&env_path, raw)?;
        }
        Ok(None) => {
            let mut buf = Vec::with_capacity(8);
            buf.extend_from_slice(FORMAT_MAGIC);
            buf.extend_from_slice(&FORMAT_VERSION.to_le_bytes());
            meta.put(&mut wtxn, META_FORMAT_KEY, &buf)
                .map_err(|source| LmdbBackendError::Io {
                    cache: cache_name.to_string(),
                    path: env_path.clone(),
                    source,
                })?;
            if meta.get(&wtxn, META_COUNT_KEY).ok().flatten().is_none() {
                meta.put(&mut wtxn, META_COUNT_KEY, &0u64.to_le_bytes())
                    .map_err(|source| LmdbBackendError::Io {
                        cache: cache_name.to_string(),
                        path: env_path.clone(),
                        source,
                    })?;
            }
        }
        Err(source) => {
            return Err(LmdbBackendError::Io {
                cache: cache_name.to_string(),
                path: env_path.clone(),
                source,
            });
        }
    }

    let count = match meta.get(&wtxn, META_COUNT_KEY) {
        Ok(Some(raw)) if raw.len() == 8 => u64::from_le_bytes(raw.try_into().unwrap_or([0; 8])),
        _ => {
            let mut n = 0u64;
            let iter = entries.iter(&wtxn).map_err(|source| LmdbBackendError::Io {
                cache: cache_name.to_string(),
                path: env_path.clone(),
                source,
            })?;
            for item in iter {
                item.map_err(|source| LmdbBackendError::Io {
                    cache: cache_name.to_string(),
                    path: env_path.clone(),
                    source,
                })?;
                n += 1;
            }
            meta.put(&mut wtxn, META_COUNT_KEY, &n.to_le_bytes())
                .map_err(|source| LmdbBackendError::Io {
                    cache: cache_name.to_string(),
                    path: env_path.clone(),
                    source,
                })?;
            n
        }
    };
    let _ = count;

    wtxn.commit().map_err(|source| LmdbBackendError::Io {
        cache: cache_name.to_string(),
        path: env_path.clone(),
        source,
    })?;

    Ok(LmdbShard {
        env_path: RwLock::new(env_path),
        env,
        meta,
        entries,
        max_entries: AtomicU64::new(max_entries),
        map_size_bytes: AtomicU64::new(map_size as u64),
    })
}

fn shard_env_path(shard: &LmdbShard) -> PathBuf {
    shard.env_path.read().clone()
}

fn shard_used_bytes(shard: &LmdbShard) -> u64 {
    let stat = shard.env.stat();
    let pages = (stat.leaf_pages as u64)
        .saturating_add(stat.branch_pages as u64)
        .saturating_add(stat.overflow_pages as u64);
    pages.saturating_mul(stat.page_size as u64)
}

fn shard_entry_count(shard: &LmdbShard, cache_name: &str) -> u64 {
    let rtxn = match shard.env.read_txn() {
        Ok(t) => t,
        Err(e) => {
            tracing::warn!(
                cache = %cache_name,
                error = %e,
                "LMDB read_txn failed for entry_count"
            );
            return 0;
        }
    };
    match shard.meta.get(&rtxn, META_COUNT_KEY) {
        Ok(Some(raw)) if raw.len() == 8 => u64::from_le_bytes(raw.try_into().unwrap_or([0; 8])),
        Ok(_) => 0,
        Err(e) => {
            tracing::warn!(
                cache = %cache_name,
                error = %e,
                "LMDB meta get failed for entry_count"
            );
            0
        }
    }
}

fn shard_key_exists(
    shard: &LmdbShard,
    cache_name: &str,
    key: &CacheKey,
) -> Result<bool, LmdbBackendError> {
    let rtxn = shard
        .env
        .read_txn()
        .map_err(|source| LmdbBackendError::Io {
            cache: cache_name.to_string(),
            path: shard_env_path(shard),
            source,
        })?;
    let v = shard
        .entries
        .get(&rtxn, key.0.as_slice())
        .map_err(|source| LmdbBackendError::Io {
            cache: cache_name.to_string(),
            path: shard_env_path(shard),
            source,
        })?;
    Ok(v.is_some())
}

fn bump_count(
    shard: &LmdbShard,
    cache_name: &str,
    wtxn: &mut heed::RwTxn<'_>,
    delta: i64,
) -> Result<(), LmdbBackendError> {
    let cur = match shard.meta.get(wtxn, META_COUNT_KEY) {
        Ok(Some(raw)) if raw.len() == 8 => u64::from_le_bytes(raw.try_into().unwrap_or([0; 8])),
        _ => 0,
    };
    let next = if delta >= 0 {
        cur.saturating_add(delta as u64)
    } else {
        cur.saturating_sub((-delta) as u64)
    };
    shard
        .meta
        .put(wtxn, META_COUNT_KEY, &next.to_le_bytes())
        .map_err(|source| LmdbBackendError::Io {
            cache: cache_name.to_string(),
            path: shard_env_path(shard),
            source,
        })?;
    Ok(())
}

fn clear_shard_entries(shard: &LmdbShard, cache_name: &str) -> Result<(), LmdbBackendError> {
    let mut wtxn = shard
        .env
        .write_txn()
        .map_err(|source| LmdbBackendError::Io {
            cache: cache_name.to_string(),
            path: shard_env_path(shard),
            source,
        })?;
    let keys: Vec<Vec<u8>> = {
        let iter = shard
            .entries
            .iter(&wtxn)
            .map_err(|source| LmdbBackendError::Io {
                cache: cache_name.to_string(),
                path: shard_env_path(shard),
                source,
            })?;
        let mut out = Vec::new();
        for item in iter {
            let (k, _) = item.map_err(|source| LmdbBackendError::Io {
                cache: cache_name.to_string(),
                path: shard_env_path(shard),
                source,
            })?;
            out.push(k.to_vec());
        }
        out
    };
    for key in &keys {
        shard
            .entries
            .delete(&mut wtxn, key)
            .map_err(|source| LmdbBackendError::Io {
                cache: cache_name.to_string(),
                path: shard_env_path(shard),
                source,
            })?;
    }
    shard
        .meta
        .put(&mut wtxn, META_COUNT_KEY, &0u64.to_le_bytes())
        .map_err(|source| LmdbBackendError::Io {
            cache: cache_name.to_string(),
            path: shard_env_path(shard),
            source,
        })?;
    wtxn.commit().map_err(|source| LmdbBackendError::Io {
        cache: cache_name.to_string(),
        path: shard_env_path(shard),
        source,
    })?;
    Ok(())
}

fn evict_victim(
    shard: &LmdbShard,
    cache_name: &str,
    policy: LmdbPolicy,
) -> Result<bool, LmdbBackendError> {
    match policy.when_full {
        LmdbWhenFull::Refuse => Ok(false),
        LmdbWhenFull::EvictOne => evict_one_arbitrary(shard, cache_name),
        LmdbWhenFull::Sample => {
            evict_from_sample(shard, cache_name, policy.sample_size.max(1) as usize)
        }
    }
}

fn evict_one_arbitrary(shard: &LmdbShard, cache_name: &str) -> Result<bool, LmdbBackendError> {
    let mut wtxn = shard
        .env
        .write_txn()
        .map_err(|source| LmdbBackendError::Io {
            cache: cache_name.to_string(),
            path: shard_env_path(shard),
            source,
        })?;
    let mut iter = shard
        .entries
        .iter(&wtxn)
        .map_err(|source| LmdbBackendError::Io {
            cache: cache_name.to_string(),
            path: shard_env_path(shard),
            source,
        })?;
    let Some(item) = iter.next() else {
        return Ok(false);
    };
    let (key, _val) = item.map_err(|source| LmdbBackendError::Io {
        cache: cache_name.to_string(),
        path: shard_env_path(shard),
        source,
    })?;
    let victim_key = key.to_vec();
    drop(iter);
    shard
        .entries
        .delete(&mut wtxn, &victim_key)
        .map_err(|source| LmdbBackendError::Io {
            cache: cache_name.to_string(),
            path: shard_env_path(shard),
            source,
        })?;
    bump_count(shard, cache_name, &mut wtxn, -1)?;
    wtxn.commit().map_err(|source| LmdbBackendError::Io {
        cache: cache_name.to_string(),
        path: shard_env_path(shard),
        source,
    })?;
    Ok(true)
}

fn evict_from_sample(
    shard: &LmdbShard,
    cache_name: &str,
    sample_size: usize,
) -> Result<bool, LmdbBackendError> {
    let rtxn = shard
        .env
        .read_txn()
        .map_err(|source| LmdbBackendError::Io {
            cache: cache_name.to_string(),
            path: shard_env_path(shard),
            source,
        })?;
    let iter = shard
        .entries
        .iter(&rtxn)
        .map_err(|source| LmdbBackendError::Io {
            cache: cache_name.to_string(),
            path: shard_env_path(shard),
            source,
        })?;
    let now_unix = unix_now();
    let mut best_expired: Option<Vec<u8>> = None;
    let mut best_soon: Option<(u64, Vec<u8>)> = None;
    let mut seen = 0usize;
    for item in iter {
        let (key, val) = item.map_err(|source| LmdbBackendError::Io {
            cache: cache_name.to_string(),
            path: shard_env_path(shard),
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
    let mut wtxn = shard
        .env
        .write_txn()
        .map_err(|source| LmdbBackendError::Io {
            cache: cache_name.to_string(),
            path: shard_env_path(shard),
            source,
        })?;
    shard
        .entries
        .delete(&mut wtxn, &victim)
        .map_err(|source| LmdbBackendError::Io {
            cache: cache_name.to_string(),
            path: shard_env_path(shard),
            source,
        })?;
    bump_count(shard, cache_name, &mut wtxn, -1)?;
    wtxn.commit().map_err(|source| LmdbBackendError::Io {
        cache: cache_name.to_string(),
        path: shard_env_path(shard),
        source,
    })?;
    Ok(true)
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

    fn test_cfg(
        path: PathBuf,
        max_entries: u64,
        when_full: LmdbWhenFull,
        shard_count: Option<u32>,
        lookup_thread_count: u32,
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
            lmdb: Some(CompiledLmdbCache {
                path,
                map_size_bytes: 2 * 1024 * 1024,
                when_full,
                sample_size: 16,
                shard_count,
                lookup_thread_count,
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
    fn resolve_explicit_wins() {
        let n = resolve_effective_shard_count(Some(4), OnDiskLayout::Sharded { n: 2 }, 8).unwrap();
        assert_eq!(n, 4);
    }

    #[test]
    fn resolve_on_disk_when_omit() {
        let n = resolve_effective_shard_count(None, OnDiskLayout::Sharded { n: 4 }, 1).unwrap();
        assert_eq!(n, 4);
        let legacy = resolve_effective_shard_count(None, OnDiskLayout::Legacy, 8).unwrap();
        assert_eq!(legacy, 1);
    }

    #[test]
    fn resolve_empty_uses_two_x_threads_and_clamp() {
        let n = resolve_effective_shard_count(None, OnDiskLayout::Empty, 2).unwrap();
        assert_eq!(n, 4);
        let clamped = resolve_effective_shard_count(None, OnDiskLayout::Empty, 100).unwrap();
        assert_eq!(clamped, MAX_LMDB_SHARD_COUNT);
        let min = resolve_effective_shard_count(None, OnDiskLayout::Empty, 0).unwrap();
        assert_eq!(min, 2);
    }

    #[test]
    fn map_size_shares_sum_to_aligned_total() {
        let sizes = per_shard_map_sizes(1_000_000_000, 4);
        let sum: usize = sizes.iter().sum();
        assert_eq!(sum, align_map_size(1_000_000_000));
        assert!(sizes.iter().all(|&s| s % 4096 == 0));
    }

    #[test]
    fn max_entries_shares_sum() {
        assert_eq!(per_shard_max_entries(0, 4), vec![0, 0, 0, 0]);
        assert_eq!(per_shard_max_entries(5, 3), vec![2, 2, 1]);
        assert_eq!(per_shard_max_entries(4, 4), vec![1, 1, 1, 1]);
    }

    #[test]
    fn open_creates_missing_parent_directories() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a").join("b").join("env");
        assert!(!path.exists());
        let cfg = test_cfg(path.clone(), 0, LmdbWhenFull::EvictOne, Some(1), 1);
        let backend = LmdbCacheBackend::open(&cfg).unwrap();
        assert!(path.is_dir());
        let now = Instant::now();
        assert!(backend.insert(CacheKey(b"k".to_vec()), entry(now, 60), now));
        assert_eq!(backend.shard_count(), 1);
        assert!(sidecar_path(&path).exists());
        assert!(shard_file_path(&path, 0).exists());
        let sidecar: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(sidecar_path(&path)).unwrap()).unwrap();
        assert_eq!(sidecar["generated_by"], "dnsconduit");
        assert!(
            sidecar["do_not_edit"]
                .as_str()
                .unwrap_or("")
                .contains("Do not edit"),
            "sidecar should warn operators not to hand-edit: {sidecar}"
        );
        drop(backend);
    }

    #[test]
    fn empty_path_defaults_to_two_x_lookup_threads() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("env");
        let cfg = test_cfg(path.clone(), 0, LmdbWhenFull::EvictOne, None, 3);
        let backend = LmdbCacheBackend::open(&cfg).unwrap();
        assert_eq!(backend.shard_count(), 6);
        drop(backend);
        // Reopen with omit reuses on-disk N (not a new 2× from a different hint).
        let cfg2 = test_cfg(path, 0, LmdbWhenFull::EvictOne, None, 1);
        let backend2 = LmdbCacheBackend::open(&cfg2).unwrap();
        assert_eq!(backend2.shard_count(), 6);
    }

    #[test]
    fn explicit_shard_count_opens_n_envs() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("env");
        let cfg = test_cfg(path.clone(), 0, LmdbWhenFull::EvictOne, Some(4), 1);
        let backend = LmdbCacheBackend::open(&cfg).unwrap();
        assert_eq!(backend.shard_count(), 4);
        for i in 0..4 {
            assert!(shard_file_path(&path, i).exists());
        }
    }

    #[test]
    fn key_routes_stably_across_get_insert() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = test_cfg(
            dir.path().join("env"),
            0,
            LmdbWhenFull::EvictOne,
            Some(4),
            1,
        );
        let backend = LmdbCacheBackend::open(&cfg).unwrap();
        let now = Instant::now();
        let key = CacheKey(b"stable-key".to_vec());
        let idx = shard_index_for_key(&key, 4);
        assert!(backend.insert(key.clone(), entry(now, 120), now));
        assert_eq!(backend.get_result(&key, now), CacheGetResult::Hit);
        assert_eq!(shard_entry_count(&backend.shards[idx], "durable"), 1);
        for (i, shard) in backend.shards.iter().enumerate() {
            if i != idx {
                assert_eq!(shard_entry_count(shard, "durable"), 0);
            }
        }
    }

    #[test]
    fn round_trip_and_persist() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("env");
        let cfg = test_cfg(path.clone(), 0, LmdbWhenFull::EvictOne, Some(2), 1);
        let backend = LmdbCacheBackend::open(&cfg).unwrap();
        let now = Instant::now();
        let key = CacheKey(b"k1".to_vec());
        assert!(backend.insert(key.clone(), entry(now, 120), now));
        assert_eq!(backend.get_result(&key, now), CacheGetResult::Hit);
        assert_eq!(backend.entry_count(), 1);
        drop(backend);

        let backend2 = LmdbCacheBackend::open(&cfg).unwrap();
        assert_eq!(backend2.shard_count(), 2);
        assert_eq!(
            backend2.get_result(&key, Instant::now()),
            CacheGetResult::Hit
        );
        assert_eq!(backend2.entry_count(), 1);
    }

    #[test]
    fn lazy_expiry_is_miss() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = test_cfg(
            dir.path().join("env"),
            0,
            LmdbWhenFull::EvictOne,
            Some(1),
            1,
        );
        let backend = LmdbCacheBackend::open(&cfg).unwrap();
        let key = CacheKey(b"stale".to_vec());
        let shard = &backend.shards[0];
        let mut wtxn = shard.env.write_txn().unwrap();
        let mut buf = Vec::new();
        let past = unix_now().saturating_sub(10);
        buf.extend_from_slice(&past.to_le_bytes());
        buf.extend_from_slice(&past.to_le_bytes());
        buf.push(0);
        let wire = sample_wire();
        buf.extend_from_slice(&(wire.len() as u32).to_le_bytes());
        buf.extend_from_slice(&wire);
        shard
            .entries
            .put(&mut wtxn, key.0.as_slice(), &buf)
            .unwrap();
        shard
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
        let cfg = test_cfg(dir.path().join("env"), 1, LmdbWhenFull::Refuse, Some(1), 1);
        let backend = LmdbCacheBackend::open(&cfg).unwrap();
        let now = Instant::now();
        assert!(backend.insert(CacheKey(b"a".to_vec()), entry(now, 60), now));
        assert!(!backend.insert(CacheKey(b"b".to_vec()), entry(now, 60), now));
        assert_eq!(backend.entry_count(), 1);
    }

    #[test]
    fn per_shard_cap_enforced_with_share_math() {
        let dir = tempfile::tempdir().unwrap();
        // Global cap 2 across 2 shards → 1 each. Fill both shards.
        let cfg = test_cfg(dir.path().join("env"), 2, LmdbWhenFull::Refuse, Some(2), 1);
        let backend = LmdbCacheBackend::open(&cfg).unwrap();
        let now = Instant::now();
        let mut inserted = 0u32;
        let mut refused = 0u32;
        for i in 0..64u32 {
            let key = CacheKey(format!("k{i}").into_bytes());
            if backend.insert(key, entry(now, 60), now) {
                inserted += 1;
            } else {
                refused += 1;
            }
        }
        assert_eq!(inserted, 2);
        assert!(refused > 0);
        assert_eq!(backend.entry_count(), 2);
        assert_eq!(per_shard_max_entries(2, 2), vec![1, 1]);
    }

    #[test]
    fn evict_one_at_entry_cap() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = test_cfg(
            dir.path().join("env"),
            1,
            LmdbWhenFull::EvictOne,
            Some(1),
            1,
        );
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
        let cfg = test_cfg(
            dir.path().join("env"),
            0,
            LmdbWhenFull::EvictOne,
            Some(1),
            1,
        );
        let backend = LmdbCacheBackend::open(&cfg).unwrap();
        let now = Instant::now();
        let key = CacheKey(b"short-ttl".to_vec());
        assert!(backend.insert(key.clone(), entry(now, 1), now));
        assert_eq!(
            backend.get_result(&key, Instant::now()),
            CacheGetResult::Hit
        );
        {
            let shard = backend.shard_for_key(&key);
            let rtxn = shard.env.read_txn().unwrap();
            let raw = shard.entries.get(&rtxn, key.0.as_slice()).unwrap().unwrap();
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
        let cfg = test_cfg(path.clone(), 0, LmdbWhenFull::EvictOne, Some(1), 1);
        let backend = LmdbCacheBackend::open(&cfg).unwrap();
        {
            let shard = &backend.shards[0];
            let mut wtxn = shard.env.write_txn().unwrap();
            let mut buf = Vec::new();
            buf.extend_from_slice(FORMAT_MAGIC);
            buf.extend_from_slice(&99u32.to_le_bytes());
            shard.meta.put(&mut wtxn, META_FORMAT_KEY, &buf).unwrap();
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

    #[test]
    fn legacy_single_env_opens_when_omit() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("legacy");
        std::fs::create_dir_all(&path).unwrap();
        // Seed a legacy directory env by opening with NO_SUB_DIR false via open_directory_env.
        let seeded =
            open_directory_env("durable", &path, align_map_size(2 * 1024 * 1024), 0).unwrap();
        {
            let now = Instant::now();
            let key = CacheKey(b"legacy-key".to_vec());
            let value = encode_entry(&entry(now, 120), now);
            let mut wtxn = seeded.env.write_txn().unwrap();
            seeded
                .entries
                .put(&mut wtxn, key.0.as_slice(), &value)
                .unwrap();
            bump_count(&seeded, "durable", &mut wtxn, 1).unwrap();
            wtxn.commit().unwrap();
        }
        drop(seeded);

        let cfg = test_cfg(path, 0, LmdbWhenFull::EvictOne, None, 8);
        let backend = LmdbCacheBackend::open(&cfg).unwrap();
        assert!(backend.layout_is_legacy());
        assert_eq!(backend.shard_count(), 1);
        assert_eq!(
            backend.get_result(&CacheKey(b"legacy-key".to_vec()), Instant::now()),
            CacheGetResult::Hit
        );
    }

    #[test]
    fn legacy_single_env_opens_when_explicit_one() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("legacy");
        std::fs::create_dir_all(&path).unwrap();
        let seeded =
            open_directory_env("durable", &path, align_map_size(2 * 1024 * 1024), 0).unwrap();
        {
            let now = Instant::now();
            let key = CacheKey(b"legacy-key".to_vec());
            let value = encode_entry(&entry(now, 120), now);
            let mut wtxn = seeded.env.write_txn().unwrap();
            seeded
                .entries
                .put(&mut wtxn, key.0.as_slice(), &value)
                .unwrap();
            bump_count(&seeded, "durable", &mut wtxn, 1).unwrap();
            wtxn.commit().unwrap();
        }
        drop(seeded);

        let cfg = test_cfg(path, 0, LmdbWhenFull::EvictOne, Some(1), 8);
        let backend = LmdbCacheBackend::open(&cfg).unwrap();
        assert!(backend.layout_is_legacy());
        assert_eq!(backend.shard_count(), 1);
        assert_eq!(
            backend.get_result(&CacheKey(b"legacy-key".to_vec()), Instant::now()),
            CacheGetResult::Hit
        );
    }

    #[test]
    fn explicit_shard_count_change_abandons_on_open() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("env");
        let cfg2 = test_cfg(path.clone(), 0, LmdbWhenFull::EvictOne, Some(2), 1);
        let backend = LmdbCacheBackend::open(&cfg2).unwrap();
        let now = Instant::now();
        let key = CacheKey(b"keep-me".to_vec());
        assert!(backend.insert(key.clone(), entry(now, 120), now));
        assert_eq!(
            backend.get_result(&key, Instant::now()),
            CacheGetResult::Hit
        );
        assert_eq!(backend.shard_count(), 2);
        drop(backend);

        let cfg8 = test_cfg(path.clone(), 0, LmdbWhenFull::EvictOne, Some(8), 1);
        let backend = LmdbCacheBackend::open(&cfg8).unwrap();
        assert_eq!(backend.shard_count(), 8);
        assert_eq!(
            backend.get_result(&key, Instant::now()),
            CacheGetResult::Miss,
            "prior entries must not migrate across explicit shard_count abandon"
        );
        assert!(sidecar_path(&path).exists());
        assert!(shard_file_path(&path, 0).exists());
        assert!(shard_file_path(&path, 7).exists());
        assert!(!shard_file_path(&path, 8).exists());
    }

    #[test]
    fn omit_reopen_keeps_on_disk_n_despite_thread_mismatch() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("env");
        let cfg = test_cfg(path.clone(), 0, LmdbWhenFull::EvictOne, Some(2), 1);
        let backend = LmdbCacheBackend::open(&cfg).unwrap();
        let now = Instant::now();
        let key = CacheKey(b"sticky".to_vec());
        assert!(backend.insert(key.clone(), entry(now, 120), now));
        drop(backend);

        // Omit shard_count with a lookup_thread_count that would prefer N=16 on empty path.
        let cfg_omit = test_cfg(path, 0, LmdbWhenFull::EvictOne, None, 8);
        let backend = LmdbCacheBackend::open(&cfg_omit).unwrap();
        assert_eq!(backend.shard_count(), 2);
        assert_eq!(
            backend.get_result(&key, Instant::now()),
            CacheGetResult::Hit
        );
    }

    #[test]
    fn shard_reopen_staging_then_finalize_empties() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("env");
        let cfg2 = test_cfg(path.clone(), 0, LmdbWhenFull::EvictOne, Some(2), 1);
        let backend = LmdbCacheBackend::open(&cfg2).unwrap();
        let now = Instant::now();
        let key = CacheKey(b"old".to_vec());
        assert!(backend.insert(key.clone(), entry(now, 120), now));
        drop(backend);

        let cfg4 = test_cfg(path.clone(), 0, LmdbWhenFull::EvictOne, Some(4), 1);
        let (replacement, operator, staging) =
            LmdbCacheBackend::open_for_shard_reopen(&cfg4).unwrap();
        assert_eq!(operator, path);
        assert!(staging.exists());
        assert_eq!(replacement.shard_count(), 4);
        // Prior layout still on disk until finalize (after prior Arc drop).
        assert!(sidecar_path(&path).exists());
        replacement
            .finalize_shard_reopen(&operator, &staging)
            .unwrap();
        assert!(!staging.exists());
        assert_eq!(replacement.path(), path);
        assert_eq!(replacement.shard_count(), 4);
        assert_eq!(
            replacement.get_result(&key, Instant::now()),
            CacheGetResult::Miss
        );
        assert!(sidecar_path(&path).exists());
        let sidecar: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(sidecar_path(&path)).unwrap()).unwrap();
        assert_eq!(sidecar["shard_count"], 4);
    }
}
