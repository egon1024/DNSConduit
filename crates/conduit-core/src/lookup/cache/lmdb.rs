//! LMDB durable answer-cache backend (`heed`).
//!
//! Hash-sharded across N independent environments under one operator `path`
//! (flat `NO_SUB_DIR` files + Conduit sidecar). Legacy single-directory envs
//! (`data.mdb` / `lock.mdb`) open as N=1 when no sidecar is present.
//!
//! Commit durability is controlled by `lmdb.sync` (`full` | `no_meta` | `none`).

use super::entry::{CacheEntry, EntryKind};
use super::key::CacheKey;
use super::memory::CacheGetResult;
use conduit_config::lookup::{
    CompiledCacheInstance, CompiledLmdbCache, LmdbSync, LmdbWhenFull, MAX_LMDB_SHARD_COUNT,
};
use conduit_metrics::MetricsHub;
use heed::types::Bytes;
use heed::{Database, Env, EnvFlags, EnvOpenOptions, FlagSetMode, MdbError};
use parking_lot::{Mutex, RwLock};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
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
    #[error("failed to set LMDB sync flags for cache '{cache}' at '{path}': {source}")]
    SetSyncFlags {
        cache: String,
        path: PathBuf,
        source: heed::Error,
    },
    #[error("invalid LMDB sync configuration for cache '{cache}': {message}")]
    InvalidSyncConfig { cache: String, message: String },
    #[error("failed to spawn LMDB periodic sync flusher thread for cache '{cache}': {source}")]
    SpawnFlusher {
        cache: String,
        source: std::io::Error,
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
    /// Set on any successful mutating commit; swapped to `false` before a
    /// `force_sync` attempt and restored to `true` if that attempt fails (see
    /// `force_sync_iter`), so a concurrent writer racing with an in-flight sync is
    /// never lost. Shared with the periodic flusher thread (if running) so both
    /// sides observe and clear the same flag.
    dirty: Arc<AtomicBool>,
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
    /// Last-success / failure-count accessors for scrape-time metrics, plus the
    /// metrics hub used to observe each completed flusher tick's duration
    /// directly (not inferred from a scrape sample).
    sync_state: Arc<SyncState>,
    /// Background periodic-sync thread; `Some` iff `policy.sync == Periodic`.
    flusher: Mutex<Option<Flusher>>,
}

#[derive(Debug, Clone, Copy)]
struct LmdbPolicy {
    when_full: LmdbWhenFull,
    sample_size: u32,
    sync: LmdbSync,
    sync_interval: Option<Duration>,
}

/// Sync observability, shared between the backend and its flusher thread.
/// `last_success`/`failures` feed scrape-time gauges/counters; `metrics` (set
/// via [`LmdbCacheBackend::set_metrics`]) lets the flusher thread observe each
/// completed tick's duration directly into the periodic sync duration
/// histogram, once per tick — not inferred from a scrape sample.
#[derive(Default)]
struct SyncState {
    last_success: RwLock<Option<Instant>>,
    failures: AtomicU64,
    metrics: RwLock<Option<Arc<MetricsHub>>>,
}

impl SyncState {
    fn record_success(&self) {
        *self.last_success.write() = Some(Instant::now());
    }

    fn record_failure(&self) {
        self.failures.fetch_add(1, Ordering::Relaxed);
    }

    /// Records a completed flusher tick's duration and, when a metrics hub is
    /// attached and metrics are enabled, observes it into
    /// `conduit_cache_lmdb_periodic_sync_duration_seconds` — the same
    /// core → metrics observe-on-completion pattern used for cache fill/eviction
    /// durations. Called exactly once per completed tick, so ticks between
    /// scrapes and idle ticks are never missed.
    fn record_tick(&self, cache_name: &str, d: Duration) {
        let hub = self.metrics.read();
        let Some(hub) = hub.as_ref() else {
            return;
        };
        if !hub.metrics_enabled() {
            return;
        }
        hub.builtin()
            .observe_periodic_sync_duration(cache_name, d.as_secs_f64());
    }
}

/// Per-shard handle held by the flusher thread: a cheap `Env` clone (heed environments
/// are `Arc`-backed internally) plus the same shared dirty flag as the owning [`LmdbShard`].
struct FlushShard {
    env: Env,
    dirty: Arc<AtomicBool>,
    path: PathBuf,
}

/// Background thread that periodically calls `force_sync` on dirty shards while
/// `lmdb.sync == periodic` (design D3/D4/D6/D7).
struct Flusher {
    stop: Arc<AtomicBool>,
    interval_ms: Arc<AtomicU64>,
    handle: Option<std::thread::JoinHandle<()>>,
}

impl Flusher {
    /// Spawns the background flusher thread. Fallible: thread spawn can fail under OS
    /// resource pressure (e.g. thread/process limits), and this must never panic the
    /// dataplane — callers propagate the error so `open` fails closed and a Hot-apply
    /// mode change rejects and keeps the prior sync mode.
    fn start(
        cache_name: String,
        shards: Vec<FlushShard>,
        interval: Duration,
        sync_state: Arc<SyncState>,
    ) -> std::io::Result<Self> {
        let stop = Arc::new(AtomicBool::new(false));
        let interval_ms = Arc::new(AtomicU64::new(interval_millis(interval)));
        let tick_in_progress = Arc::new(AtomicBool::new(false));
        let thread_stop = Arc::clone(&stop);
        let thread_interval_ms = Arc::clone(&interval_ms);
        let handle = std::thread::Builder::new()
            .name(format!("conduit-lmdb-sync-{cache_name}"))
            .spawn(move || {
                flusher_loop(
                    &cache_name,
                    &shards,
                    &thread_stop,
                    &thread_interval_ms,
                    &tick_in_progress,
                    &sync_state,
                );
            })?;
        Ok(Self {
            stop,
            interval_ms,
            handle: Some(handle),
        })
    }

    fn set_interval(&self, interval: Duration) {
        self.interval_ms
            .store(interval_millis(interval), Ordering::Relaxed);
    }

    /// Stop the thread and join it, bounding the wait so `Drop`/Hot-apply cannot hang
    /// forever on a stuck `force_sync` syscall (e.g. slow disk).
    ///
    /// Returns `true` if the flusher thread was confirmed to have exited within the
    /// timeout, `false` if the wait timed out first. Callers MUST check this return
    /// value before running a direct best-effort `force_sync_dirty_shards`: if the
    /// thread is still inside `force_sync` when we give up waiting, a direct call
    /// from `Drop`/mode-change would run concurrently with (or stack behind) that
    /// still-running sync, defeating the whole point of joining first. On a `false`
    /// return the caller should skip the direct sync and log a warning instead.
    ///
    /// On timeout the underlying `std::thread::JoinHandle` is intentionally dropped
    /// (not joined further) by the watcher thread once the real join eventually
    /// completes — the flusher thread itself was already asked to stop via `self.stop`
    /// and will exit on its own as soon as its in-flight `force_sync` returns; we just
    /// stop waiting for it here so shutdown stays bounded. This can leave the OS thread
    /// running detached for the duration of one stuck syscall, which is an accepted
    /// trade-off for a bounded `Drop`.
    #[must_use]
    fn stop_and_join(mut self) -> bool {
        self.stop.store(true, Ordering::Relaxed);
        let Some(handle) = self.handle.take() else {
            return true;
        };
        let (tx, rx) = std::sync::mpsc::channel::<()>();
        std::thread::spawn(move || {
            let _ = handle.join();
            let _ = tx.send(());
        });
        if rx.recv_timeout(Duration::from_secs(5)).is_err() {
            tracing::warn!(
                "LMDB periodic sync flusher thread did not stop within timeout; abandoning join"
            );
            false
        } else {
            true
        }
    }
}

fn interval_millis(interval: Duration) -> u64 {
    (interval.as_millis() as u64).max(1)
}

fn flusher_loop(
    cache_name: &str,
    shards: &[FlushShard],
    stop: &AtomicBool,
    interval_ms: &AtomicU64,
    tick_in_progress: &AtomicBool,
    sync_state: &SyncState,
) {
    const POLL_CHUNK: Duration = Duration::from_millis(50);
    loop {
        let mut waited = Duration::ZERO;
        loop {
            if stop.load(Ordering::Relaxed) {
                return;
            }
            let target = Duration::from_millis(interval_ms.load(Ordering::Relaxed).max(1));
            if waited >= target {
                break;
            }
            let chunk = (target - waited).min(POLL_CHUNK);
            std::thread::sleep(chunk);
            waited += chunk;
        }
        if stop.load(Ordering::Relaxed) {
            return;
        }
        // This flag is local to (and only ever touched by) this single loop thread,
        // so `compare_exchange` never actually observes `true` here today — each
        // iteration clears it before waiting again, and `run_flush_tick` below runs
        // to completion synchronously on this thread. It is not shared with a direct
        // `force_sync_dirty_shards` call: overlap between a tick and a direct call is
        // instead prevented by construction, because `reconcile_flusher` and `Drop`
        // only invoke `force_sync_dirty_shards` directly after `Flusher::stop_and_join`
        // reports a *confirmed* join of this thread — if the join times out instead
        // (thread still possibly mid-`force_sync`), those callers skip the direct call
        // and log a warning rather than risk an overlapping/stacked sync. Kept as a
        // defensive guard against a future refactor of this loop reintroducing real
        // concurrency.
        if tick_in_progress
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Relaxed)
            .is_err()
        {
            continue;
        }
        let t0 = Instant::now();
        run_flush_tick(cache_name, shards, sync_state);
        sync_state.record_tick(cache_name, t0.elapsed());
        tick_in_progress.store(false, Ordering::Release);
    }
}

fn run_flush_tick(cache_name: &str, shards: &[FlushShard], sync_state: &SyncState) {
    let _ = force_sync_iter(
        cache_name,
        sync_state,
        shards
            .iter()
            .map(|s| (&s.env, s.dirty.as_ref(), s.path.clone())),
    );
}

/// Shared force-sync loop used by both the flusher thread (via [`FlushShard`] clones)
/// and [`LmdbCacheBackend::force_sync_dirty_shards`] (via live shard fields). Visits
/// every dirty shard even after a failure; returns the first error, if any.
fn force_sync_iter<'a>(
    cache_name: &str,
    sync_state: &SyncState,
    shards: impl Iterator<Item = (&'a Env, &'a AtomicBool, PathBuf)>,
) -> Result<(), LmdbBackendError> {
    let mut first_err = None;
    for (env, dirty, path) in shards {
        // Swap the flag to false *before* calling `force_sync`, not after a successful
        // sync. A writer that commits and marks the shard dirty while `force_sync` is
        // in flight races with a store-based clear: syncing then unconditionally
        // storing `false` afterwards would clobber that concurrent write's flag back
        // to clean even though `force_sync` may not have covered it. Swapping first
        // means a concurrent writer's `store(true)` (which happens after our swap)
        // always wins and leaves the shard correctly marked dirty for the next tick.
        if !dirty.swap(false, Ordering::AcqRel) {
            continue;
        }
        match env.force_sync() {
            Ok(()) => {
                sync_state.record_success();
            }
            Err(source) => {
                // Not durably flushed: restore the obligation so the shard stays
                // dirty and gets retried, even if a concurrent writer already
                // re-dirtied it in the meantime (a redundant future sync is fine).
                dirty.store(true, Ordering::Release);
                tracing::error!(
                    cache = %cache_name,
                    path = %path.display(),
                    error = %source,
                    "LMDB periodic force_sync failed for shard"
                );
                sync_state.record_failure();
                if first_err.is_none() {
                    first_err = Some(LmdbBackendError::Io {
                        cache: cache_name.to_string(),
                        path,
                        source,
                    });
                }
            }
        }
    }
    match first_err {
        Some(e) => Err(e),
        None => Ok(()),
    }
}

fn sync_env_flags(sync: LmdbSync) -> EnvFlags {
    match sync {
        LmdbSync::Full => EnvFlags::empty(),
        LmdbSync::NoMeta => EnvFlags::NO_META_SYNC,
        LmdbSync::Periodic | LmdbSync::None => EnvFlags::NO_SYNC,
    }
}

/// Apply sync durability flags on an already-open environment (Hot apply).
///
/// # Safety
///
/// Caller must ensure exclusive access to `set_flags` for this env (single-threaded
/// wrt other `set_flags` calls), matching heed/LMDB requirements.
unsafe fn apply_sync_flags_to_env(env: &Env, sync: LmdbSync) -> Result<(), heed::Error> {
    // Clear both durability flags, then enable the mode's flag (if any).
    env.set_flags(EnvFlags::NO_SYNC, FlagSetMode::Disable)?;
    env.set_flags(EnvFlags::NO_META_SYNC, FlagSetMode::Disable)?;
    match sync {
        LmdbSync::Full => Ok(()),
        LmdbSync::NoMeta => env.set_flags(EnvFlags::NO_META_SYNC, FlagSetMode::Enable),
        LmdbSync::Periodic | LmdbSync::None => {
            env.set_flags(EnvFlags::NO_SYNC, FlagSetMode::Enable)
        }
    }
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
            let shard = open_directory_env(cache_name, &path, map_sizes[0], caps[0], lmdb.sync)?;
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
                    lmdb.sync,
                )?);
            }
            write_sidecar(&path, effective_n)?;
            (LayoutKind::Sharded, shards)
        };

        let aligned_total: u64 = shards
            .iter()
            .map(|s| s.map_size_bytes.load(Ordering::Relaxed))
            .sum();

        let backend = Self {
            cache_name: cache_name.to_string(),
            path: RwLock::new(path),
            layout,
            shards,
            total_map_size_bytes: AtomicU64::new(aligned_total),
            total_max_entries: AtomicU64::new(max_entries),
            policy: RwLock::new(LmdbPolicy {
                when_full: lmdb.when_full,
                sample_size: lmdb.sample_size,
                sync: lmdb.sync,
                sync_interval: lmdb.sync_interval,
            }),
            sync_state: Arc::new(SyncState::default()),
            flusher: Mutex::new(None),
        };
        if lmdb.sync == LmdbSync::Periodic {
            if let Some(interval) = lmdb.sync_interval {
                *backend.flusher.lock() = Some(backend.start_flusher(interval)?);
            }
        }
        Ok(backend)
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
        let mut policy = self.policy.write();
        policy.when_full = when_full;
        policy.sample_size = sample_size;
    }

    pub fn sync_mode(&self) -> LmdbSync {
        self.policy.read().sync
    }

    /// Hot-apply `lmdb.sync` (and, for `periodic`, `sync_interval`) on every open shard
    /// environment, starting/stopping/reconfiguring the background flusher as needed.
    pub fn apply_sync_config(
        &self,
        sync: LmdbSync,
        interval: Option<Duration>,
    ) -> Result<(), LmdbBackendError> {
        if sync == LmdbSync::Periodic && interval.is_none() {
            return Err(LmdbBackendError::InvalidSyncConfig {
                cache: self.cache_name.clone(),
                message: "sync=periodic requires sync_interval".into(),
            });
        }
        if sync != LmdbSync::Periodic && interval.is_some() {
            return Err(LmdbBackendError::InvalidSyncConfig {
                cache: self.cache_name.clone(),
                message: format!(
                    "sync_interval must only be set when sync=periodic (got sync={})",
                    sync.as_str()
                ),
            });
        }

        let (prev_sync, prev_interval) = {
            let policy = self.policy.read();
            (policy.sync, policy.sync_interval)
        };
        if prev_sync == sync && prev_interval == interval {
            return Ok(());
        }

        if prev_sync != sync {
            for shard in &self.shards {
                // SAFETY: registry reconcile is single-threaded for this instance; no
                // concurrent set_flags on these envs.
                unsafe { apply_sync_flags_to_env(&shard.env, sync) }.map_err(|source| {
                    LmdbBackendError::SetSyncFlags {
                        cache: self.cache_name.clone(),
                        path: shard.env_path.read().clone(),
                        source,
                    }
                })?;
            }
        }

        {
            let mut policy = self.policy.write();
            policy.sync = sync;
            policy.sync_interval = interval;
        }

        if let Err(e) = self.reconcile_flusher(sync, interval) {
            // Reject the Hot apply and keep the prior mode: restore the policy fields and,
            // if we already flipped env sync flags above, flip them back so the backend
            // does not end up in a mixed state (new durability flags, old flusher/mode).
            {
                let mut policy = self.policy.write();
                policy.sync = prev_sync;
                policy.sync_interval = prev_interval;
            }
            if prev_sync != sync {
                for shard in &self.shards {
                    // SAFETY: same single-threaded-reconcile guarantee as the forward
                    // apply above.
                    if let Err(revert_err) =
                        unsafe { apply_sync_flags_to_env(&shard.env, prev_sync) }
                    {
                        tracing::error!(
                            cache = %self.cache_name,
                            error = %revert_err,
                            "LMDB failed to revert sync flags after flusher start failure; \
                             env durability flags may not match the reported prior sync mode"
                        );
                    }
                }
            }
            tracing::error!(
                cache = %self.cache_name,
                error = %e,
                "LMDB periodic sync flusher failed to start; Hot apply rejected, keeping prior sync mode"
            );
            return Err(e);
        }

        tracing::info!(
            cache = %self.cache_name,
            from_sync = prev_sync.as_str(),
            to_sync = sync.as_str(),
            from_interval_ms = prev_interval.map(interval_millis),
            to_interval_ms = interval.map(interval_millis),
            "LMDB sync mode updated"
        );
        Ok(())
    }

    fn start_flusher(&self, interval: Duration) -> Result<Flusher, LmdbBackendError> {
        let shards: Vec<FlushShard> = self
            .shards
            .iter()
            .map(|s| FlushShard {
                env: s.env.clone(),
                dirty: Arc::clone(&s.dirty),
                path: shard_env_path(s),
            })
            .collect();
        Flusher::start(
            self.cache_name.clone(),
            shards,
            interval,
            Arc::clone(&self.sync_state),
        )
        .map_err(|source| LmdbBackendError::SpawnFlusher {
            cache: self.cache_name.clone(),
            source,
        })
    }

    /// Start, stop, or reconfigure the background flusher to match `sync`/`interval`.
    /// Stopping while dirty shards remain performs one best-effort final sync so a
    /// mode change (e.g. `periodic` → `none`) does not silently drop already-buffered
    /// writes.
    ///
    /// Fallible: starting a new flusher thread can fail (see [`Flusher::start`]). Callers
    /// (`apply_sync_config`, `open_at`) must treat an `Err` as a rejected Hot apply / failed
    /// open rather than proceeding with a half-applied sync mode.
    fn reconcile_flusher(
        &self,
        sync: LmdbSync,
        interval: Option<Duration>,
    ) -> Result<(), LmdbBackendError> {
        let mut guard = self.flusher.lock();
        match (sync, interval) {
            (LmdbSync::Periodic, Some(iv)) => match guard.as_ref() {
                Some(existing) => {
                    existing.set_interval(iv);
                    Ok(())
                }
                None => {
                    let flusher = self.start_flusher(iv)?;
                    *guard = Some(flusher);
                    Ok(())
                }
            },
            _ => {
                if let Some(flusher) = guard.take() {
                    drop(guard);
                    if flusher.stop_and_join() {
                        if let Err(e) = self.force_sync_dirty_shards() {
                            tracing::warn!(
                                cache = %self.cache_name,
                                error = %e,
                                "LMDB best-effort sync after leaving periodic mode failed"
                            );
                        }
                    } else {
                        // Thread did not confirm exit within the join timeout: it may
                        // still be inside `force_sync`. Do not start a second, overlapping
                        // `force_sync_dirty_shards` on top of it.
                        tracing::warn!(
                            cache = %self.cache_name,
                            "LMDB periodic sync flusher did not stop in time after leaving \
                             periodic mode; skipping best-effort final sync to avoid an \
                             overlapping force_sync"
                        );
                    }
                }
                Ok(())
            }
        }
    }

    /// Force-sync every shard currently marked dirty, clearing the flag on success.
    /// Used by the periodic flusher tick and by `Drop`/mode-change best-effort syncs.
    pub fn force_sync_dirty_shards(&self) -> Result<(), LmdbBackendError> {
        force_sync_iter(
            &self.cache_name,
            &self.sync_state,
            self.shards
                .iter()
                .map(|s| (&s.env, s.dirty.as_ref(), shard_env_path(s))),
        )
    }

    /// Timestamp of the most recent successful `force_sync` (any shard), for scrape-time
    /// metrics. `None` until the first periodic sync succeeds.
    pub fn last_sync_success(&self) -> Option<Instant> {
        *self.sync_state.last_success.read()
    }

    /// Total count of failed `force_sync` calls across all shards, for scrape-time metrics.
    pub fn sync_failure_count(&self) -> u64 {
        self.sync_state.failures.load(Ordering::Relaxed)
    }

    /// Attaches the metrics hub used to observe each completed periodic flusher
    /// tick's duration directly (see [`SyncState::record_tick`]). Safe to call
    /// before, during, or after the flusher thread is running: the flusher reads
    /// the current value on every tick.
    pub fn set_metrics(&self, metrics: Arc<MetricsHub>) {
        *self.sync_state.metrics.write() = Some(metrics);
    }

    #[cfg(test)]
    pub(crate) fn test_dirty_flags(&self) -> Vec<bool> {
        self.shards
            .iter()
            .map(|s| s.dirty.load(Ordering::Acquire))
            .collect()
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

    /// Insert an entry. `stored` is false when refused under `when_full: refuse`.
    pub fn insert(
        &self,
        key: CacheKey,
        entry: CacheEntry,
        now: Instant,
    ) -> super::backend::InsertOutcome {
        let mut eviction_secs = 0.0_f64;
        let mut evictions = 0_u64;
        match self.insert_inner(key, entry, now, true, &mut eviction_secs, &mut evictions) {
            Ok(stored) => super::backend::InsertOutcome {
                stored,
                eviction_secs,
                evictions,
            },
            Err(e) => {
                tracing::error!(
                    cache = %self.cache_name,
                    path = %self.path.read().display(),
                    error = %e,
                    "LMDB insert failed"
                );
                super::backend::InsertOutcome {
                    stored: false,
                    eviction_secs,
                    evictions,
                }
            }
        }
    }

    fn insert_inner(
        &self,
        key: CacheKey,
        entry: CacheEntry,
        now: Instant,
        allow_retry: bool,
        eviction_secs: &mut f64,
        evictions: &mut u64,
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
                        if !timed_evict_victim(
                            shard,
                            &self.cache_name,
                            policy,
                            eviction_secs,
                            evictions,
                        )? {
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
                shard.dirty.store(true, Ordering::Release);
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
                        if !timed_evict_victim(
                            shard,
                            &self.cache_name,
                            policy,
                            eviction_secs,
                            evictions,
                        )? {
                            tracing::error!(
                                cache = %self.cache_name,
                                "LMDB map full; eviction found no victim"
                            );
                            return Ok(false);
                        }
                        self.insert_inner(key, entry, now, false, eviction_secs, evictions)
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
        shard.dirty.store(true, Ordering::Release);
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

impl Drop for LmdbCacheBackend {
    fn drop(&mut self) {
        let flusher = self.flusher.lock().take();
        // Only run the direct best-effort final sync after a *confirmed* join: if the
        // flusher thread timed out (still possibly inside `force_sync`), calling
        // `force_sync_dirty_shards` here would run concurrently with — or stack behind
        // — that still-running sync. `joined` is `true` when there was no flusher to
        // begin with, so the final sync below still runs in the (much more common)
        // non-periodic-or-already-stopped case.
        let joined = flusher.map(Flusher::stop_and_join).unwrap_or(true);
        let was_periodic = self.policy.read().sync == LmdbSync::Periodic;
        if was_periodic {
            if joined {
                if let Err(e) = self.force_sync_dirty_shards() {
                    tracing::warn!(
                        cache = %self.cache_name,
                        error = %e,
                        "LMDB best-effort final sync on close failed"
                    );
                }
            } else {
                tracing::warn!(
                    cache = %self.cache_name,
                    "LMDB periodic sync flusher did not stop in time on close; skipping \
                     final sync because the thread may still be mid-sync"
                );
            }
        }
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
    sync: LmdbSync,
) -> Result<LmdbShard, LmdbBackendError> {
    // SAFETY: path is operator-controlled; registry holds one backend Arc per name.
    // Sync durability flags (`NO_SYNC` / `NO_META_SYNC`) are intentional operator knobs.
    let env = unsafe {
        let mut opts = EnvOpenOptions::new();
        opts.map_size(map_size).max_dbs(2);
        let flags = sync_env_flags(sync);
        if !flags.is_empty() {
            opts.flags(flags);
        }
        opts.open(path)
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
    sync: LmdbSync,
) -> Result<LmdbShard, LmdbBackendError> {
    // SAFETY: path is operator-controlled; NO_SUB_DIR opens a file path (not a directory).
    // Sync durability flags are intentional operator knobs.
    let env = unsafe {
        let mut opts = EnvOpenOptions::new();
        opts.map_size(map_size).max_dbs(2);
        opts.flags(EnvFlags::NO_SUB_DIR | sync_env_flags(sync));
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
        dirty: Arc::new(AtomicBool::new(false)),
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
    shard.dirty.store(true, Ordering::Release);
    Ok(())
}

fn timed_evict_victim(
    shard: &LmdbShard,
    cache_name: &str,
    policy: LmdbPolicy,
    eviction_secs: &mut f64,
    evictions: &mut u64,
) -> Result<bool, LmdbBackendError> {
    let t0 = Instant::now();
    let ok = evict_victim(shard, cache_name, policy)?;
    if ok {
        *eviction_secs += t0.elapsed().as_secs_f64();
        *evictions += 1;
    }
    Ok(ok)
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
    shard.dirty.store(true, Ordering::Release);
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
    shard.dirty.store(true, Ordering::Release);
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
        EvictionMode, LmdbSync, OnHitResponseRules,
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
                sync: LmdbSync::Full,
                sync_interval: None,
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
        assert!(
            backend
                .insert(CacheKey(b"k".to_vec()), entry(now, 60), now)
                .stored
        );
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
    fn sync_hot_apply_updates_mode() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("env");
        let mut cfg = test_cfg(path, 0, LmdbWhenFull::EvictOne, Some(2), 1);
        cfg.lmdb.as_mut().unwrap().sync = LmdbSync::Full;
        let backend = LmdbCacheBackend::open(&cfg).unwrap();
        assert_eq!(backend.sync_mode(), LmdbSync::Full);
        backend.apply_sync_config(LmdbSync::NoMeta, None).unwrap();
        assert_eq!(backend.sync_mode(), LmdbSync::NoMeta);
        backend.apply_sync_config(LmdbSync::None, None).unwrap();
        assert_eq!(backend.sync_mode(), LmdbSync::None);
        backend.apply_sync_config(LmdbSync::Full, None).unwrap();
        assert_eq!(backend.sync_mode(), LmdbSync::Full);
        let now = Instant::now();
        assert!(
            backend
                .insert(CacheKey(b"k".to_vec()), entry(now, 60), now)
                .stored
        );
    }

    #[test]
    fn periodic_flusher_clears_dirty_after_interval() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("env");
        let mut cfg = test_cfg(path, 0, LmdbWhenFull::EvictOne, Some(2), 1);
        cfg.lmdb.as_mut().unwrap().sync = LmdbSync::Periodic;
        cfg.lmdb.as_mut().unwrap().sync_interval = Some(Duration::from_millis(250));
        let backend = LmdbCacheBackend::open(&cfg).unwrap();

        let now = Instant::now();
        assert!(
            backend
                .insert(CacheKey(b"a".to_vec()), entry(now, 60), now)
                .stored
        );
        assert!(
            backend
                .insert(CacheKey(b"b".to_vec()), entry(now, 60), now)
                .stored
        );
        assert!(
            backend.test_dirty_flags().iter().any(|d| *d),
            "at least one shard should be dirty immediately after insert"
        );

        std::thread::sleep(Duration::from_millis(500));

        assert!(
            backend.test_dirty_flags().iter().all(|d| !*d),
            "flusher tick should have cleared dirty flags after the interval elapsed"
        );
        assert!(backend.last_sync_success().is_some());
        assert_eq!(backend.sync_failure_count(), 0);
    }

    #[test]
    fn apply_sync_config_rejects_invalid_coupling() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("env");
        let cfg = test_cfg(path, 0, LmdbWhenFull::EvictOne, Some(1), 1);
        let backend = LmdbCacheBackend::open(&cfg).unwrap();

        // Periodic without an interval is rejected.
        let err = backend
            .apply_sync_config(LmdbSync::Periodic, None)
            .unwrap_err();
        assert!(err.to_string().contains("requires sync_interval"));

        // Non-periodic with an interval set is rejected.
        let err = backend
            .apply_sync_config(LmdbSync::Full, Some(Duration::from_secs(1)))
            .unwrap_err();
        assert!(err
            .to_string()
            .contains("must only be set when sync=periodic"));
    }

    #[test]
    fn hot_apply_full_to_periodic_to_none_starts_and_stops_flusher() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("env");
        let mut cfg = test_cfg(path, 0, LmdbWhenFull::EvictOne, Some(1), 1);
        cfg.lmdb.as_mut().unwrap().sync = LmdbSync::Full;
        let backend = LmdbCacheBackend::open(&cfg).unwrap();
        assert!(backend.flusher.lock().is_none());

        // Full -> Periodic starts the flusher.
        backend
            .apply_sync_config(LmdbSync::Periodic, Some(Duration::from_millis(250)))
            .unwrap();
        assert!(backend.flusher.lock().is_some());
        assert_eq!(backend.sync_mode(), LmdbSync::Periodic);

        let now = Instant::now();
        assert!(
            backend
                .insert(CacheKey(b"a".to_vec()), entry(now, 60), now)
                .stored
        );
        std::thread::sleep(Duration::from_millis(500));
        assert!(
            backend.test_dirty_flags().iter().all(|d| !*d),
            "flusher must have synced the dirty shard while periodic"
        );

        // Periodic -> None stops the flusher and performs a best-effort final sync.
        assert!(
            backend
                .insert(CacheKey(b"b".to_vec()), entry(now, 60), now)
                .stored
        );
        backend.apply_sync_config(LmdbSync::None, None).unwrap();
        assert!(backend.flusher.lock().is_none());
        assert_eq!(backend.sync_mode(), LmdbSync::None);
        assert!(
            backend.test_dirty_flags().iter().all(|d| !*d),
            "stopping periodic mode should best-effort sync remaining dirty shards"
        );
    }

    #[test]
    fn stop_and_join_reports_timeout_when_thread_does_not_exit_in_time() {
        // Directly exercises `Flusher::stop_and_join`'s timeout branch: spawn a
        // "flusher" thread that ignores `stop` (simulating a stuck `force_sync`
        // syscall) well past the internal 5s join bound, and confirm the call
        // returns `false` (timed out) rather than blocking forever or reporting a
        // confirmed join. Callers (`Drop`/`reconcile_flusher`) rely on this `false`
        // to skip the direct best-effort `force_sync_dirty_shards` call.
        let stop = Arc::new(AtomicBool::new(false));
        let interval_ms = Arc::new(AtomicU64::new(50));
        let handle = std::thread::Builder::new()
            .name("test-stuck-flusher".into())
            .spawn(|| {
                std::thread::sleep(Duration::from_secs(7));
            })
            .unwrap();
        let flusher = Flusher {
            stop,
            interval_ms,
            handle: Some(handle),
        };
        let started = Instant::now();
        let joined = flusher.stop_and_join();
        assert!(
            !joined,
            "stop_and_join must report a timeout (false), not a confirmed join"
        );
        assert!(
            started.elapsed() < Duration::from_secs(7),
            "stop_and_join must bound its wait rather than blocking for the full stuck duration"
        );
    }

    #[test]
    fn hot_apply_interval_change_updates_without_reopen() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("env");
        let mut cfg = test_cfg(path, 0, LmdbWhenFull::EvictOne, Some(1), 1);
        cfg.lmdb.as_mut().unwrap().sync = LmdbSync::Periodic;
        cfg.lmdb.as_mut().unwrap().sync_interval = Some(Duration::from_secs(60));
        let backend = LmdbCacheBackend::open(&cfg).unwrap();

        let now = Instant::now();
        assert!(
            backend
                .insert(CacheKey(b"a".to_vec()), entry(now, 60), now)
                .stored
        );
        // A 60s interval would not fire within this test's sleep window; shrinking the
        // interval via Hot apply (no reopen) must make the flusher pick it up promptly.
        backend
            .apply_sync_config(LmdbSync::Periodic, Some(Duration::from_millis(250)))
            .unwrap();

        std::thread::sleep(Duration::from_millis(500));
        assert!(
            backend.test_dirty_flags().iter().all(|d| !*d),
            "shrinking sync_interval via Hot apply must take effect without reopening the env"
        );
    }

    #[test]
    fn open_with_no_meta_sync() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("env");
        let mut cfg = test_cfg(path, 0, LmdbWhenFull::EvictOne, Some(1), 1);
        cfg.lmdb.as_mut().unwrap().sync = LmdbSync::NoMeta;
        let backend = LmdbCacheBackend::open(&cfg).unwrap();
        assert_eq!(backend.sync_mode(), LmdbSync::NoMeta);
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
        assert!(backend.insert(key.clone(), entry(now, 120), now).stored);
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
        assert!(backend.insert(key.clone(), entry(now, 120), now).stored);
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
        assert!(
            backend
                .insert(CacheKey(b"a".to_vec()), entry(now, 60), now)
                .stored
        );
        assert!(
            !backend
                .insert(CacheKey(b"b".to_vec()), entry(now, 60), now)
                .stored
        );
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
            if backend.insert(key, entry(now, 60), now).stored {
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
        assert!(
            backend
                .insert(CacheKey(b"a".to_vec()), entry(now, 60), now)
                .stored
        );
        assert!(
            backend
                .insert(CacheKey(b"b".to_vec()), entry(now, 60), now)
                .stored
        );
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
        assert!(backend.insert(key.clone(), entry(now, 1), now).stored);
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
        let seeded = open_directory_env(
            "durable",
            &path,
            align_map_size(2 * 1024 * 1024),
            0,
            LmdbSync::Full,
        )
        .unwrap();
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
        let seeded = open_directory_env(
            "durable",
            &path,
            align_map_size(2 * 1024 * 1024),
            0,
            LmdbSync::Full,
        )
        .unwrap();
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
        assert!(backend.insert(key.clone(), entry(now, 120), now).stored);
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
        assert!(backend.insert(key.clone(), entry(now, 120), now).stored);
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
        assert!(backend.insert(key.clone(), entry(now, 120), now).stored);
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
