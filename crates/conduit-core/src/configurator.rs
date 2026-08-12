//! Serialized configuration apply path (phase 5).
//!
//! All snapshot installs flow through the Configurator so phase 5b can enqueue Rhai proposals
//! on the same channel without refactoring gRPC handlers.

use crate::snapshot::{RuntimeSnapshot, SnapshotStore};
use conduit_config::{
    clear_overlay, is_overlay_patch_empty, load_yaml, merge_file_and_overlay,
    merge_overlay_patches, synthesize_overlay, validate, validate_overlay_patch, EffectiveConfig,
    ValidationResult,
};
use conduit_events::EventHub;
use conduit_metrics::{MetricsExportController, MetricsHub};
use conduit_proto::config::{
    Backend, CacheInstance, CacheNegativeConfig, CacheOnHitConfig, CacheTruncatedUdpConfig, Config,
    DataSource, DataSourceLimits, EventSinkFilters, MetricsConfig,
};
use std::fmt;
use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tokio::sync::{mpsc, oneshot};
use tokio::task::JoinHandle;

/// Origin of a configuration proposal (extensible for phase 5b / autoscaler).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProposalSource {
    Grpc,
    File,
    Sighup,
}

impl fmt::Display for ProposalSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ProposalSource::Grpc => write!(f, "grpc"),
            ProposalSource::File => write!(f, "file"),
            ProposalSource::Sighup => write!(f, "sighup"),
        }
    }
}

/// How an API overlay patch is applied to the accumulated overlay layer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OverlayApplyMode {
    #[default]
    Merge,
    Replace,
    Clear,
}

/// Typed overlay-hot config mutation.
///
/// Applied inside the Configurator: `desired ← effective` → mutate →
/// `synthesize_overlay(file, desired)` → Replace overlay → validate/compile/swap.
#[derive(Debug, Clone, PartialEq)]
pub enum ConfigPrimitive {
    SetBackendWeight {
        pool: String,
        /// Backend name when configured; otherwise address (`host:port`).
        backend: String,
        weight: u32,
    },
    RemoveBackend {
        pool: String,
        backend: String,
    },
    AddBackend {
        pool: String,
        backend: Backend,
    },
    SetOrchestratorLimits {
        max_attempts: Option<u32>,
        max_txn_duration_ms: Option<u32>,
    },
    UpsertDataSource {
        source: Box<DataSource>,
    },
    RemoveDataSource {
        name: String,
    },
    SetDataSourceLimits {
        limits: DataSourceLimits,
    },
    SetEventSinkFilters {
        name: String,
        filters: EventSinkFilters,
    },
    SetEventSinkEmit {
        name: String,
        emit: Vec<String>,
        extra_fields: Vec<String>,
        extra_tags: Vec<String>,
        extra_fields_set: bool,
        extra_tags_set: bool,
    },
    SetRhaiLimits {
        max_operations: Option<u64>,
        max_call_depth: Option<u32>,
        hook_timeout_ms: Option<u32>,
    },
    PatchMetrics {
        metrics: Box<MetricsConfig>,
    },
    SetCacheMaxEntries {
        name: String,
        max_entries: u64,
    },
    SetCacheLmdbHot {
        name: String,
        when_full: Option<String>,
        sample_size: Option<u32>,
        sync: Option<String>,
        sync_interval: Option<String>,
        map_size_bytes: Option<u64>,
    },
    SetCachePolicyHot {
        name: String,
        negative_cache: Option<CacheNegativeConfig>,
        on_hit: Option<CacheOnHitConfig>,
        truncated_udp: Option<CacheTruncatedUdpConfig>,
        rotate_rrset_on_serve: Option<bool>,
    },
}

/// Configuration change submitted to the Configurator.
#[derive(Debug, Clone)]
pub struct PolicyProposal {
    pub source: ProposalSource,
    /// gRPC overlay patch (interpretation depends on [`Self::overlay_mode`]).
    /// Ignored when [`Self::primitive`] is `Some`.
    pub overlay: Option<Config>,
    pub overlay_mode: OverlayApplyMode,
    /// When set, replaces the file baseline (reload from disk).
    pub file_reload: Option<Config>,
    pub correlation_id: Option<String>,
    /// When set, document overlay fields are ignored; use synthesize+Replace path.
    pub primitive: Option<ConfigPrimitive>,
}

/// One status/effect note attached to an apply outcome (mirrors proto `ConfigApplyStatusNote`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApplyStatusNote {
    pub kind: String,
    pub message: String,
}

/// Outcome of a single apply attempt.
#[derive(Debug, Clone)]
pub struct ApplyResult {
    pub ok: bool,
    pub errors: Vec<String>,
    pub generation: u64,
    /// Extensible effect / pending-reconcile notes; empty is OK for fully hot applies.
    pub notes: Vec<ApplyStatusNote>,
}

struct ProposalEnvelope {
    proposal: PolicyProposal,
    reply: oneshot::Sender<ApplyResult>,
}

/// Shared state for the Configurator loop.
pub struct ConfiguratorState {
    pub config_path: PathBuf,
    pub base_dir: Option<PathBuf>,
    /// Metrics hub for hot-swapping metrics plan on config reload.
    pub metrics_hub: Option<Arc<MetricsHub>>,
    /// Export controller for hot-rebinding Prometheus/OTLP sinks.
    pub export_controller: Option<Arc<MetricsExportController>>,
    /// Event hub for passing to export controller commit.
    pub events: Option<Arc<EventHub>>,
}

/// Handle used by gRPC, SIGHUP, and tests to enqueue proposals.
#[derive(Clone)]
pub struct ConfiguratorHandle {
    tx: mpsc::Sender<ProposalEnvelope>,
}

impl ConfiguratorHandle {
    pub async fn propose(&self, proposal: PolicyProposal) -> ApplyResult {
        let (reply_tx, reply_rx) = oneshot::channel();
        if self
            .tx
            .send(ProposalEnvelope {
                proposal,
                reply: reply_tx,
            })
            .await
            .is_err()
        {
            return ApplyResult {
                ok: false,
                errors: vec!["configurator stopped".into()],
                generation: 0,
                notes: vec![],
            };
        }
        reply_rx.await.unwrap_or(ApplyResult {
            ok: false,
            errors: vec!["configurator dropped reply".into()],
            generation: 0,
            notes: vec![],
        })
    }

    pub async fn apply_overlay(
        &self,
        overlay: Option<Config>,
        mode: OverlayApplyMode,
        correlation_id: Option<String>,
    ) -> ApplyResult {
        self.propose(PolicyProposal {
            source: ProposalSource::Grpc,
            overlay,
            overlay_mode: mode,
            file_reload: None,
            correlation_id,
            primitive: None,
        })
        .await
    }

    /// Enqueue a typed config primitive (synthesize overlay + Replace).
    pub async fn apply_primitive(
        &self,
        primitive: ConfigPrimitive,
        correlation_id: Option<String>,
    ) -> ApplyResult {
        self.propose(PolicyProposal {
            source: ProposalSource::Grpc,
            overlay: None,
            overlay_mode: OverlayApplyMode::Replace,
            file_reload: None,
            correlation_id,
            primitive: Some(primitive),
        })
        .await
    }

    pub async fn reload_from_file(&self, source: ProposalSource) -> ApplyResult {
        self.propose(PolicyProposal {
            source,
            overlay: None,
            overlay_mode: OverlayApplyMode::Merge,
            file_reload: None,
            correlation_id: None,
            primitive: None,
        })
        .await
    }
}

/// Spawn product of the Configurator background task.
pub struct ConfiguratorSpawn {
    handle: ConfiguratorHandle,
    task: JoinHandle<()>,
    /// Sender to inject export controller after dataplane starts.
    state_tx: mpsc::Sender<ConfiguratorStateUpdate>,
}

/// Dynamic updates to configurator state (sent after spawn).
enum ConfiguratorStateUpdate {
    ExportController(Arc<MetricsExportController>, Arc<EventHub>),
}

impl ConfiguratorSpawn {
    pub fn handle(&self) -> ConfiguratorHandle {
        self.handle.clone()
    }

    /// Wire the export controller into the configurator state for hot-rebind.
    ///
    /// Called by RuntimeSupervisor after the dataplane is started and the
    /// export controller is created. The controller needs EventHub, which
    /// only exists after dataplane start.
    pub fn set_export_controller(
        &mut self,
        controller: Arc<MetricsExportController>,
        events: Arc<EventHub>,
    ) {
        let _ = self
            .state_tx
            .try_send(ConfiguratorStateUpdate::ExportController(
                controller, events,
            ));
    }

    pub async fn shutdown(self) {
        // Drop *both* senders so the background select loop can exit.
        // G4 added `state_tx` for late export-controller injection; leaving it
        // alive after dropping only the proposal handle left `state_rx.recv()`
        // pending forever and hung process shutdown (SIGINT never completed).
        drop(self.handle);
        drop(self.state_tx);
        match self.task.await {
            Ok(()) => {}
            Err(e) if e.is_panic() => std::panic::resume_unwind(e.into_panic()),
            Err(e) => tracing::warn!(error = %e, "configurator task failed"),
        }
    }
}

/// Spawn the Configurator consumer task. Returns a handle for enqueueing proposals.
pub fn spawn(
    store: Arc<SnapshotStore>,
    effective: Arc<Mutex<EffectiveConfig>>,
    state: ConfiguratorState,
) -> ConfiguratorSpawn {
    let (tx, mut rx) = mpsc::channel::<ProposalEnvelope>(64);
    let (state_tx, mut state_rx) = mpsc::channel::<ConfiguratorStateUpdate>(4);
    let handle = ConfiguratorHandle { tx: tx.clone() };

    let task = tokio::spawn(async move {
        // Mutable state that can be updated after spawn (e.g., export controller).
        let mut state = state;

        loop {
            tokio::select! {
                // Handle state updates (e.g., export controller injection).
                Some(update) = state_rx.recv() => {
                    match update {
                        ConfiguratorStateUpdate::ExportController(controller, events) => {
                            state.export_controller = Some(controller);
                            state.events = Some(events);
                            tracing::debug!("export controller wired into configurator");
                        }
                    }
                }
                // Handle proposal envelopes.
                Some(envelope) = rx.recv() => {
                    let result = apply_proposal(&store, &effective, &state, envelope.proposal).await;
                    let _ = envelope.reply.send(result);
                }
                else => break,
            }
        }
    });

    ConfiguratorSpawn {
        handle,
        task,
        state_tx,
    }
}

async fn apply_proposal(
    store: &SnapshotStore,
    effective: &Mutex<EffectiveConfig>,
    state: &ConfiguratorState,
    proposal: PolicyProposal,
) -> ApplyResult {
    let prev = store.load();

    let merged = match prepare_effective(effective, state, &proposal) {
        Ok(cfg) => cfg,
        Err(errors) => {
            return ApplyResult {
                ok: false,
                errors,
                generation: store.generation(),
                notes: vec![],
            };
        }
    };

    // Compile metrics config for export controller prepare and hub apply.
    let (compiled, _) = conduit_metrics::compile_from_config(&merged);

    // Prepare export rebind BEFORE installing snapshot: if Prometheus bind fails,
    // reject the apply without mutating any runtime state.
    let pending_export = if let Some(ref controller) = state.export_controller {
        match controller.prepare(&compiled).await {
            Ok(pending) => Some(pending),
            Err(e) => {
                tracing::warn!(error = %e, "export rebind prepare failed; rejecting apply");
                return ApplyResult {
                    ok: false,
                    errors: vec![e],
                    generation: store.generation(),
                    notes: vec![],
                };
            }
        }
    } else {
        None
    };

    match store.install_validated_with_base(merged.clone(), state.base_dir.as_deref()) {
        Ok(()) => {
            let new = store.load();
            log_config_applied(proposal.source, new.generation, &prev, &new);

            // Hot-swap metrics plan if hub is configured.
            if let Some(ref hub) = state.metrics_hub {
                hub.apply_compiled(compiled);
                tracing::debug!(generation = new.generation, "metrics plan hot-swapped");
            }

            // Commit export rebind AFTER successful snapshot install.
            if let (Some(pending), Some(ref controller), Some(ref events)) =
                (pending_export, &state.export_controller, &state.events)
            {
                controller.commit(pending, events.clone()).await;
            }

            ApplyResult {
                ok: true,
                errors: vec![],
                generation: new.generation,
                // Fully hot document applies leave notes empty; pending-restart
                // honesty notes are added when warm/cold paths need them.
                notes: vec![],
            }
        }
        Err(errors) => ApplyResult {
            ok: false,
            errors,
            generation: store.generation(),
            notes: vec![],
        },
    }
}

fn prepare_effective(
    effective: &Mutex<EffectiveConfig>,
    state: &ConfiguratorState,
    proposal: &PolicyProposal,
) -> Result<Config, Vec<String>> {
    let mut eff = effective
        .lock()
        .map_err(|_| vec!["effective config lock poisoned".into()])?;

    // Mutate a working copy so validate failures leave last-good file/overlay intact.
    let mut working = EffectiveConfig {
        file: eff.file.clone(),
        overlay: eff.overlay.clone(),
    };

    if let Some(file_cfg) = &proposal.file_reload {
        working.file = file_cfg.clone();
        clear_overlay(&mut working);
    } else if matches!(
        proposal.source,
        ProposalSource::File | ProposalSource::Sighup
    ) && proposal.overlay.is_none()
        && proposal.primitive.is_none()
    {
        let yaml = fs::read_to_string(&state.config_path)
            .map_err(|e| vec![format!("reading config {:?}: {e}", state.config_path)])?;
        let file_cfg = load_yaml(&yaml).map_err(|e| vec![e.to_string()])?;
        working.file = file_cfg;
        clear_overlay(&mut working);
    }

    if proposal.primitive.is_some() {
        apply_primitive_proposal(&mut working, proposal)?;
    } else if proposal.source == ProposalSource::Grpc {
        apply_overlay_mode(&mut working, proposal)?;
    }

    let merged = working.effective();
    let ValidationResult { ok, errors } = validate(&merged);
    if !ok {
        return Err(errors);
    }
    // Commit only after validate succeeds.
    *eff = working;
    Ok(merged)
}

/// desired ← effective → mutate → synthesize → Replace overlay (no sparse Merge).
fn apply_primitive_proposal(
    eff: &mut EffectiveConfig,
    proposal: &PolicyProposal,
) -> Result<(), Vec<String>> {
    let primitive = proposal
        .primitive
        .as_ref()
        .expect("apply_primitive_proposal requires primitive");
    let mut desired = eff.effective();
    mutate_desired(&mut desired, primitive)?;
    let overlay = synthesize_overlay(&eff.file, &desired).map_err(|e| vec![e.to_string()])?;
    // Same install rules as OverlayApplyMode::Replace.
    reject_forbidden_overlay_fields(&overlay)?;
    if is_overlay_patch_empty(&overlay) {
        clear_overlay(eff);
    } else {
        merge_file_and_overlay(&eff.file, &overlay).map_err(|e| vec![e.to_string()])?;
        eff.overlay = Some(overlay);
    }
    Ok(())
}

fn mutate_desired(desired: &mut Config, primitive: &ConfigPrimitive) -> Result<(), Vec<String>> {
    match primitive {
        ConfigPrimitive::SetBackendWeight {
            pool,
            backend,
            weight,
        } => {
            let p = find_pool_mut(desired, pool)?;
            let b = find_backend_mut(p, backend)?;
            b.weight = Some(*weight);
            Ok(())
        }
        ConfigPrimitive::RemoveBackend { pool, backend } => {
            let p = find_pool_mut(desired, pool)?;
            let idx = find_backend_index(p, backend)?;
            p.backends.remove(idx);
            Ok(())
        }
        ConfigPrimitive::AddBackend { pool, backend } => {
            let p = find_pool_mut(desired, pool)?;
            if backend_identity_exists(p, backend) {
                return Err(vec![format!(
                    "pool '{pool}' already has a backend matching the add identity"
                )]);
            }
            if backend.address.is_empty() {
                return Err(vec!["add backend requires a non-empty address".into()]);
            }
            let mut added = backend.clone();
            added.remove = None;
            p.backends.push(added);
            Ok(())
        }
        ConfigPrimitive::SetOrchestratorLimits {
            max_attempts,
            max_txn_duration_ms,
        } => {
            if max_attempts.is_none() && max_txn_duration_ms.is_none() {
                return Err(vec![
                    "SetOrchestratorLimits requires max_attempts and/or max_txn_duration_ms".into(),
                ]);
            }
            let mut orch = desired.orchestrator.unwrap_or_default();
            if let Some(v) = max_attempts {
                orch.max_attempts = *v;
            }
            if let Some(v) = max_txn_duration_ms {
                orch.max_txn_duration_ms = *v;
            }
            desired.orchestrator = Some(orch);
            Ok(())
        }
        ConfigPrimitive::UpsertDataSource { source } => {
            if source.name.is_empty() {
                return Err(vec!["data source name is required".into()]);
            }
            if let Some(existing) = desired
                .data_sources
                .iter_mut()
                .find(|s| s.name == source.name)
            {
                *existing = (**source).clone();
            } else {
                desired.data_sources.push((**source).clone());
            }
            Ok(())
        }
        ConfigPrimitive::RemoveDataSource { name } => {
            if name.is_empty() {
                return Err(vec!["data source name is required".into()]);
            }
            let before = desired.data_sources.len();
            desired.data_sources.retain(|s| s.name != *name);
            if desired.data_sources.len() == before {
                return Err(vec![format!("unknown data source '{name}'")]);
            }
            Ok(())
        }
        ConfigPrimitive::SetDataSourceLimits { limits } => {
            desired.data_source_limits = Some(*limits);
            Ok(())
        }
        ConfigPrimitive::SetEventSinkFilters { name, filters } => {
            let sink = find_event_sink_mut(desired, name)?;
            sink.filters = Some(filters.clone());
            Ok(())
        }
        ConfigPrimitive::SetEventSinkEmit {
            name,
            emit,
            extra_fields,
            extra_tags,
            extra_fields_set,
            extra_tags_set,
        } => {
            let sink = find_event_sink_mut(desired, name)?;
            sink.emit = emit.clone();
            if *extra_fields_set {
                sink.extra_fields = extra_fields.clone();
            }
            if *extra_tags_set {
                sink.extra_tags = extra_tags.clone();
            }
            Ok(())
        }
        ConfigPrimitive::SetRhaiLimits {
            max_operations,
            max_call_depth,
            hook_timeout_ms,
        } => {
            if max_operations.is_none() && max_call_depth.is_none() && hook_timeout_ms.is_none() {
                return Err(vec![
                    "SetRhaiLimits requires at least one of max_operations, max_call_depth, hook_timeout_ms"
                        .into(),
                ]);
            }
            let mut rhai = desired.rhai.unwrap_or_default();
            if let Some(v) = max_operations {
                rhai.max_operations = *v;
            }
            if let Some(v) = max_call_depth {
                rhai.max_call_depth = *v;
            }
            if let Some(v) = hook_timeout_ms {
                rhai.hook_timeout_ms = *v;
            }
            desired.rhai = Some(rhai);
            Ok(())
        }
        ConfigPrimitive::PatchMetrics { metrics } => {
            let patch = Config {
                schema_version: desired.schema_version,
                metrics: Some((**metrics).clone()),
                ..Default::default()
            };
            *desired = merge_file_and_overlay(desired, &patch).map_err(|e| vec![e.to_string()])?;
            Ok(())
        }
        ConfigPrimitive::SetCacheMaxEntries { name, max_entries } => {
            let cache = find_cache_mut(desired, name)?;
            cache.max_entries = Some(*max_entries);
            Ok(())
        }
        ConfigPrimitive::SetCacheLmdbHot {
            name,
            when_full,
            sample_size,
            sync,
            sync_interval,
            map_size_bytes,
        } => {
            if when_full.is_none()
                && sample_size.is_none()
                && sync.is_none()
                && sync_interval.is_none()
                && map_size_bytes.is_none()
            {
                return Err(vec![
                    "SetCacheLmdbHot requires at least one LMDB hot field".into()
                ]);
            }
            let cache = find_cache_mut(desired, name)?;
            if cache.r#type != "lmdb" {
                return Err(vec![format!(
                    "cache '{name}' type is '{}' (SetCacheLmdbHot requires lmdb)",
                    cache.r#type
                )]);
            }
            let mut lmdb = cache.lmdb.clone().unwrap_or_default();
            if let Some(v) = when_full {
                lmdb.when_full = Some(v.clone());
            }
            if let Some(v) = sample_size {
                lmdb.sample_size = Some(*v);
            }
            if let Some(v) = sync {
                lmdb.sync = Some(v.clone());
            }
            if let Some(v) = sync_interval {
                lmdb.sync_interval = Some(v.clone());
            }
            if let Some(v) = map_size_bytes {
                lmdb.map_size_bytes = *v;
            }
            cache.lmdb = Some(lmdb);
            Ok(())
        }
        ConfigPrimitive::SetCachePolicyHot {
            name,
            negative_cache,
            on_hit,
            truncated_udp,
            rotate_rrset_on_serve,
        } => {
            if negative_cache.is_none()
                && on_hit.is_none()
                && truncated_udp.is_none()
                && rotate_rrset_on_serve.is_none()
            {
                return Err(vec![
                    "SetCachePolicyHot requires at least one policy field".into()
                ]);
            }
            let cache = find_cache_mut(desired, name)?;
            if let Some(v) = negative_cache {
                cache.negative_cache = Some(*v);
            }
            if let Some(v) = on_hit {
                cache.on_hit = Some(v.clone());
            }
            if let Some(v) = truncated_udp {
                cache.truncated_udp = Some(*v);
            }
            if let Some(v) = rotate_rrset_on_serve {
                cache.rotate_rrset_on_serve = Some(*v);
            }
            Ok(())
        }
    }
}

fn find_event_sink_mut<'a>(
    cfg: &'a mut Config,
    name: &str,
) -> Result<&'a mut conduit_proto::config::EventSink, Vec<String>> {
    if name.is_empty() {
        return Err(vec!["event sink name is required".into()]);
    }
    let events = cfg
        .events
        .as_mut()
        .ok_or_else(|| vec!["no events section in effective config".into()])?;
    events
        .sinks
        .iter_mut()
        .find(|s| event_sink_name(s) == name)
        .ok_or_else(|| vec![format!("unknown event sink '{name}'")])
}

fn event_sink_name(sink: &conduit_proto::config::EventSink) -> &str {
    if let Some(n) = sink.name.as_deref().filter(|n| !n.is_empty()) {
        n
    } else if !sink.export_id.is_empty() {
        sink.export_id.as_str()
    } else {
        ""
    }
}

fn find_cache_mut<'a>(
    cfg: &'a mut Config,
    name: &str,
) -> Result<&'a mut CacheInstance, Vec<String>> {
    if name.is_empty() {
        return Err(vec!["cache name is required".into()]);
    }
    cfg.caches
        .iter_mut()
        .find(|c| c.name == name)
        .ok_or_else(|| vec![format!("unknown cache '{name}'")])
}

fn find_pool_mut<'a>(
    cfg: &'a mut Config,
    pool_name: &str,
) -> Result<&'a mut conduit_proto::config::Pool, Vec<String>> {
    cfg.pools
        .iter_mut()
        .find(|p| p.name == pool_name)
        .ok_or_else(|| vec![format!("unknown pool '{pool_name}'")])
}

fn find_backend_mut<'a>(
    pool: &'a mut conduit_proto::config::Pool,
    backend_id: &str,
) -> Result<&'a mut Backend, Vec<String>> {
    let idx = find_backend_index(pool, backend_id)?;
    Ok(&mut pool.backends[idx])
}

fn find_backend_index(
    pool: &conduit_proto::config::Pool,
    backend_id: &str,
) -> Result<usize, Vec<String>> {
    if let Some(idx) = pool
        .backends
        .iter()
        .position(|b| b.name.as_deref() == Some(backend_id))
    {
        return Ok(idx);
    }
    if let Some(idx) = pool.backends.iter().position(|b| b.address == backend_id) {
        return Ok(idx);
    }
    Err(vec![format!(
        "pool '{}' has no backend named or addressed '{}'",
        pool.name, backend_id
    )])
}

fn backend_identity_exists(pool: &conduit_proto::config::Pool, backend: &Backend) -> bool {
    if let Some(name) = backend.name.as_ref().filter(|n| !n.is_empty()) {
        if pool
            .backends
            .iter()
            .any(|b| b.name.as_deref() == Some(name.as_str()))
        {
            return true;
        }
    }
    !backend.address.is_empty() && pool.backends.iter().any(|b| b.address == backend.address)
}

fn apply_overlay_mode(
    eff: &mut EffectiveConfig,
    proposal: &PolicyProposal,
) -> Result<(), Vec<String>> {
    match proposal.overlay_mode {
        OverlayApplyMode::Clear => {
            clear_overlay(eff);
            Ok(())
        }
        OverlayApplyMode::Replace => {
            let patch = proposal
                .overlay
                .as_ref()
                .ok_or_else(|| vec!["replace apply requires overlay".into()])?;
            reject_forbidden_overlay_fields(patch)?;
            if is_overlay_patch_empty(patch) {
                clear_overlay(eff);
            } else {
                merge_file_and_overlay(&eff.file, patch).map_err(|e| vec![e.to_string()])?;
                eff.overlay = Some(patch.clone());
            }
            Ok(())
        }
        OverlayApplyMode::Merge => {
            let Some(patch) = &proposal.overlay else {
                return Err(vec!["merge apply requires overlay".into()]);
            };
            reject_forbidden_overlay_fields(patch)?;
            if is_overlay_patch_empty(patch) {
                return Ok(());
            }
            let new_overlay = match &eff.overlay {
                Some(existing) => {
                    merge_overlay_patches(existing, patch).map_err(|e| vec![e.to_string()])?
                }
                None => patch.clone(),
            };
            merge_file_and_overlay(&eff.file, &new_overlay).map_err(|e| vec![e.to_string()])?;
            eff.overlay = Some(new_overlay);
            Ok(())
        }
    }
}

fn reject_forbidden_overlay_fields(patch: &Config) -> Result<(), Vec<String>> {
    let validation = validate_overlay_patch(patch);
    if validation.ok {
        Ok(())
    } else {
        Err(validation.errors)
    }
}

/// Operator-facing log after a successful snapshot swap.
pub fn log_config_applied(
    source: ProposalSource,
    generation: u64,
    prev: &RuntimeSnapshot,
    new: &RuntimeSnapshot,
) {
    tracing::info!(generation, %source, "config applied");
    log_runtime_diff(&prev.config, &new.config);
    log_pending_reconcile(prev, new);
}

fn log_pending_reconcile(prev: &RuntimeSnapshot, new: &RuntimeSnapshot) {
    let listeners_changed = prev.config.listeners != new.config.listeners;
    let forward_changed = prev.config.forward != new.config.forward;
    if listeners_changed {
        tracing::info!(
            "listeners: pending (restart required) — snapshot updated, sockets not rebound"
        );
    }
    if forward_changed {
        tracing::info!(
            "forward egress: pending (restart required) — snapshot updated, sockets not rebound"
        );
    }
    for (name, new_inst) in &new.lookup.cache_instances {
        let Some(prev_inst) = prev.lookup.cache_instances.get(name) else {
            continue;
        };
        if prev_inst.memory.shard_count != new_inst.memory.shard_count {
            tracing::info!(
                cache = %name,
                "cache memory.shard_count: pending (restart required) — snapshot updated, shard layout unchanged"
            );
        }
    }
}

fn log_runtime_diff(prev: &Config, new: &Config) {
    let prev_pools = prev.pools.len();
    let new_pools = new.pools.len();
    if prev_pools != new_pools {
        tracing::info!(prev_pools, new_pools, "pools: count changed");
    }
    for new_pool in &new.pools {
        if let Some(old_pool) = prev.pools.iter().find(|p| p.name == new_pool.name) {
            if old_pool.backends != new_pool.backends {
                tracing::info!(pool = %new_pool.name, "pool: backends changed");
            }
        } else {
            tracing::info!(pool = %new_pool.name, "pool: added");
        }
    }
    let prev_sinks = prev.events.as_ref().map(|e| e.sinks.len()).unwrap_or(0);
    let new_sinks = new.events.as_ref().map(|e| e.sinks.len()).unwrap_or(0);
    if prev_sinks != new_sinks {
        tracing::info!(prev_sinks, new_sinks, "observation: sink count changed");
    }
    let prev_rules = prev.rules.as_ref().map(|r| r.rules.len()).unwrap_or(0);
    let new_rules = new.rules.as_ref().map(|r| r.rules.len()).unwrap_or(0);
    if prev_rules != new_rules {
        tracing::info!(prev_rules, new_rules, "rules: count changed");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use conduit_config::load_yaml;
    #[tokio::test]
    async fn apply_overlay_changes_weight_and_generation() {
        let yaml = include_str!("../../../tests/fixtures/config/minimal.yaml");
        let file_cfg = load_yaml(yaml).unwrap();
        let store = Arc::new(SnapshotStore::new(RuntimeSnapshot::from_config(
            file_cfg.clone(),
        )));
        let effective = Arc::new(Mutex::new(EffectiveConfig::new(file_cfg.clone())));
        let state = ConfiguratorState {
            config_path: PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../../tests/fixtures/config/minimal.yaml"),
            base_dir: Some(
                PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/config"),
            ),
            metrics_hub: None,
            export_controller: None,
            events: None,
        };
        let handle = spawn(store.clone(), effective, state).handle();

        let overlay = pool_weight_patch(&file_cfg, 42);

        let result = handle
            .apply_overlay(Some(overlay), OverlayApplyMode::Merge, None)
            .await;
        assert!(result.ok, "{:?}", result.errors);
        assert_eq!(store.load().config.pools[0].backends[0].weight, Some(42));
        assert_eq!(store.generation(), result.generation);
        assert!(store.generation() >= 1);
    }

    #[tokio::test]
    async fn invalid_overlay_rejected() {
        let yaml = include_str!("../../../tests/fixtures/config/minimal.yaml");
        let file_cfg = load_yaml(yaml).unwrap();
        let store = Arc::new(SnapshotStore::new(RuntimeSnapshot::from_config(
            file_cfg.clone(),
        )));
        let effective = Arc::new(Mutex::new(EffectiveConfig::new(file_cfg.clone())));
        let state = ConfiguratorState {
            config_path: PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../../tests/fixtures/config/minimal.yaml"),
            base_dir: Some(
                PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/config"),
            ),
            metrics_hub: None,
            export_controller: None,
            events: None,
        };
        let handle = spawn(store.clone(), effective, state).handle();
        let gen0 = store.generation();

        let mut overlay = listeners_only_patch(2);
        overlay.listeners.as_mut().unwrap().threads = 0;

        let result = handle
            .apply_overlay(Some(overlay), OverlayApplyMode::Merge, None)
            .await;
        assert!(!result.ok);
        assert_eq!(store.generation(), gen0);
        assert_eq!(store.load().config.pools[0].backends[0].weight, Some(100));
    }

    #[tokio::test]
    async fn reload_clears_overlay() {
        let yaml = include_str!("../../../tests/fixtures/config/minimal.yaml");
        let file_cfg = load_yaml(yaml).unwrap();
        let store = Arc::new(SnapshotStore::new(RuntimeSnapshot::from_config(
            file_cfg.clone(),
        )));
        let effective = Arc::new(Mutex::new(EffectiveConfig::new(file_cfg.clone())));
        let state = ConfiguratorState {
            config_path: PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../../tests/fixtures/config/minimal.yaml"),
            base_dir: Some(
                PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/config"),
            ),
            metrics_hub: None,
            export_controller: None,
            events: None,
        };
        let handle = spawn(store.clone(), effective.clone(), state).handle();

        handle
            .apply_overlay(
                Some(pool_weight_patch(&file_cfg, 42)),
                OverlayApplyMode::Merge,
                None,
            )
            .await;
        assert_eq!(store.load().config.pools[0].backends[0].weight, Some(42));

        let result = handle.reload_from_file(ProposalSource::Sighup).await;
        assert!(result.ok, "{:?}", result.errors);
        assert_eq!(store.load().config.pools[0].backends[0].weight, Some(100));
        let eff = effective.lock().unwrap();
        assert!(eff.overlay.is_none());
    }

    #[tokio::test]
    async fn interleaved_merge_apply_and_primitive_preserves_unrelated_overlay() {
        let file_cfg = two_named_backend_config();
        let store = Arc::new(SnapshotStore::new(RuntimeSnapshot::from_config(
            file_cfg.clone(),
        )));
        let effective = Arc::new(Mutex::new(EffectiveConfig::new(file_cfg.clone())));
        let state = ConfiguratorState {
            config_path: PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../../tests/fixtures/config/minimal.yaml"),
            base_dir: Some(
                PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/config"),
            ),
            metrics_hub: None,
            export_controller: None,
            events: None,
        };
        let handle = spawn(store.clone(), effective.clone(), state).handle();

        // Document merge: set primary weight only.
        let mut overlay = Config {
            schema_version: 1,
            ..Default::default()
        };
        let mut pool = file_cfg.pools[0].clone();
        pool.backends = vec![Backend {
            name: Some("primary".into()),
            weight: Some(11),
            ..Default::default()
        }];
        overlay.pools = vec![pool];
        let merge = handle
            .apply_overlay(Some(overlay), OverlayApplyMode::Merge, None)
            .await;
        assert!(merge.ok, "{:?}", merge.errors);

        // Primitive: set secondary weight; must not drop primary's overlay weight.
        let prim = handle
            .apply_primitive(
                ConfigPrimitive::SetBackendWeight {
                    pool: "edge".into(),
                    backend: "secondary".into(),
                    weight: 22,
                },
                None,
            )
            .await;
        assert!(prim.ok, "{:?}", prim.errors);

        let snap = store.load();
        let backends = &snap.config.pools[0].backends;
        let primary = backends
            .iter()
            .find(|b| b.name.as_deref() == Some("primary"))
            .expect("primary");
        let secondary = backends
            .iter()
            .find(|b| b.name.as_deref() == Some("secondary"))
            .expect("secondary");
        assert_eq!(primary.weight, Some(11));
        assert_eq!(secondary.weight, Some(22));

        // Overlay layer still carries both deltas (synthesize+Replace).
        let eff = effective.lock().unwrap();
        let ov = eff.overlay.as_ref().expect("overlay present");
        assert!(
            ov.pools.iter().any(|p| {
                p.backends
                    .iter()
                    .any(|b| b.name.as_deref() == Some("primary") && b.weight == Some(11))
            }),
            "overlay should retain primary weight: {ov:?}"
        );
    }

    #[tokio::test]
    async fn primitive_validate_failure_keeps_last_good() {
        let yaml = include_str!("../../../tests/fixtures/config/minimal.yaml");
        let file_cfg = load_yaml(yaml).unwrap();
        let store = Arc::new(SnapshotStore::new(RuntimeSnapshot::from_config(
            file_cfg.clone(),
        )));
        let effective = Arc::new(Mutex::new(EffectiveConfig::new(file_cfg.clone())));
        let state = ConfiguratorState {
            config_path: PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../../tests/fixtures/config/minimal.yaml"),
            base_dir: Some(
                PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/config"),
            ),
            metrics_hub: None,
            export_controller: None,
            events: None,
        };
        let handle = spawn(store.clone(), effective.clone(), state).handle();

        // First: a successful weight change so last-good is not the file baseline.
        let ok = handle
            .apply_primitive(
                ConfigPrimitive::SetBackendWeight {
                    pool: "default".into(),
                    backend: "127.0.0.1:5300".into(),
                    weight: 55,
                },
                None,
            )
            .await;
        assert!(ok.ok, "{:?}", ok.errors);
        let gen_ok = store.generation();
        assert_eq!(store.load().config.pools[0].backends[0].weight, Some(55));

        // Removing the only backend leaves an empty pool → validate fails.
        let bad = handle
            .apply_primitive(
                ConfigPrimitive::RemoveBackend {
                    pool: "default".into(),
                    backend: "127.0.0.1:5300".into(),
                },
                None,
            )
            .await;
        assert!(!bad.ok);
        assert!(
            bad.errors.iter().any(|e| e.contains("no backends")),
            "{:?}",
            bad.errors
        );
        assert_eq!(store.generation(), gen_ok);
        assert_eq!(store.load().config.pools[0].backends[0].weight, Some(55));
        assert_eq!(store.load().config.pools[0].backends.len(), 1);
    }

    fn two_named_backend_config() -> Config {
        let yaml = include_str!("../../../tests/fixtures/config/minimal.yaml");
        let mut cfg = load_yaml(yaml).unwrap();
        cfg.pools[0].name = "edge".into();
        cfg.pools[0].backends = vec![
            Backend {
                name: Some("primary".into()),
                address: "127.0.0.1:5300".into(),
                weight: Some(100),
                ..Default::default()
            },
            Backend {
                name: Some("secondary".into()),
                address: "127.0.0.1:5301".into(),
                weight: Some(100),
                ..Default::default()
            },
        ];
        cfg
    }

    fn pool_weight_patch(file_cfg: &Config, weight: u32) -> Config {
        let mut pool = file_cfg.pools[0].clone();
        pool.backends[0].weight = Some(weight);
        Config {
            schema_version: 1,
            pools: vec![pool],
            ..Default::default()
        }
    }

    fn listeners_only_patch(threads: u32) -> Config {
        Config {
            schema_version: 1,
            listeners: Some(conduit_proto::config::ListenersConfig {
                threads,
                reuse_port: true,
                rcvbuf: 0,
                sndbuf: 0,
                listeners: vec![],
            }),
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn merge_apply_accumulates_overlay_fields() {
        let yaml = include_str!("../../../tests/fixtures/config/minimal.yaml");
        let file_cfg = load_yaml(yaml).unwrap();
        let store = Arc::new(SnapshotStore::new(RuntimeSnapshot::from_config(
            file_cfg.clone(),
        )));
        let effective = Arc::new(Mutex::new(EffectiveConfig::new(file_cfg.clone())));
        let state = ConfiguratorState {
            config_path: PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../../tests/fixtures/config/minimal.yaml"),
            base_dir: Some(
                PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/config"),
            ),
            metrics_hub: None,
            export_controller: None,
            events: None,
        };
        let handle = spawn(store.clone(), effective, state).handle();

        handle
            .apply_overlay(
                Some(pool_weight_patch(&file_cfg, 50)),
                OverlayApplyMode::Merge,
                None,
            )
            .await;
        handle
            .apply_overlay(Some(listeners_only_patch(4)), OverlayApplyMode::Merge, None)
            .await;

        let snap = store.load();
        assert_eq!(snap.config.pools[0].backends[0].weight, Some(50));
        assert_eq!(snap.config.listeners.as_ref().unwrap().threads, 4);
    }

    #[tokio::test]
    async fn replace_apply_drops_prior_overlay_fields() {
        let yaml = include_str!("../../../tests/fixtures/config/minimal.yaml");
        let file_cfg = load_yaml(yaml).unwrap();
        let store = Arc::new(SnapshotStore::new(RuntimeSnapshot::from_config(
            file_cfg.clone(),
        )));
        let effective = Arc::new(Mutex::new(EffectiveConfig::new(file_cfg.clone())));
        let state = ConfiguratorState {
            config_path: PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../../tests/fixtures/config/minimal.yaml"),
            base_dir: Some(
                PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/config"),
            ),
            metrics_hub: None,
            export_controller: None,
            events: None,
        };
        let handle = spawn(store.clone(), effective, state).handle();

        handle
            .apply_overlay(
                Some(pool_weight_patch(&file_cfg, 50)),
                OverlayApplyMode::Merge,
                None,
            )
            .await;
        handle
            .apply_overlay(
                Some(listeners_only_patch(4)),
                OverlayApplyMode::Replace,
                None,
            )
            .await;

        let snap = store.load();
        assert_eq!(snap.config.pools[0].backends[0].weight, Some(100));
        assert_eq!(snap.config.listeners.as_ref().unwrap().threads, 4);
    }

    #[tokio::test]
    async fn clear_apply_drops_overlay_without_reload() {
        let yaml = include_str!("../../../tests/fixtures/config/minimal.yaml");
        let file_cfg = load_yaml(yaml).unwrap();
        let store = Arc::new(SnapshotStore::new(RuntimeSnapshot::from_config(
            file_cfg.clone(),
        )));
        let effective = Arc::new(Mutex::new(EffectiveConfig::new(file_cfg.clone())));
        let state = ConfiguratorState {
            config_path: PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../../tests/fixtures/config/minimal.yaml"),
            base_dir: Some(
                PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/config"),
            ),
            metrics_hub: None,
            export_controller: None,
            events: None,
        };
        let handle = spawn(store.clone(), effective.clone(), state).handle();

        handle
            .apply_overlay(
                Some(pool_weight_patch(&file_cfg, 50)),
                OverlayApplyMode::Merge,
                None,
            )
            .await;
        let result = handle
            .apply_overlay(None, OverlayApplyMode::Clear, None)
            .await;
        assert!(result.ok, "{:?}", result.errors);
        assert_eq!(store.load().config.pools[0].backends[0].weight, Some(100));
        assert!(effective.lock().unwrap().overlay.is_none());
    }

    #[tokio::test]
    async fn replace_empty_patch_clears_overlay() {
        let yaml = include_str!("../../../tests/fixtures/config/minimal.yaml");
        let file_cfg = load_yaml(yaml).unwrap();
        let store = Arc::new(SnapshotStore::new(RuntimeSnapshot::from_config(
            file_cfg.clone(),
        )));
        let effective = Arc::new(Mutex::new(EffectiveConfig::new(file_cfg.clone())));
        let state = ConfiguratorState {
            config_path: PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../../tests/fixtures/config/minimal.yaml"),
            base_dir: Some(
                PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/config"),
            ),
            metrics_hub: None,
            export_controller: None,
            events: None,
        };
        let handle = spawn(store.clone(), effective.clone(), state).handle();

        handle
            .apply_overlay(
                Some(pool_weight_patch(&file_cfg, 50)),
                OverlayApplyMode::Merge,
                None,
            )
            .await;
        handle
            .apply_overlay(
                Some(Config {
                    schema_version: 1,
                    ..Default::default()
                }),
                OverlayApplyMode::Replace,
                None,
            )
            .await;

        assert_eq!(store.load().config.pools[0].backends[0].weight, Some(100));
        assert!(effective.lock().unwrap().overlay.is_none());
    }

    #[tokio::test]
    async fn overlay_patch_with_rules_rejected() {
        use conduit_proto::config::RulesConfig;

        let yaml = include_str!("../../../tests/fixtures/config/minimal.yaml");
        let file_cfg = load_yaml(yaml).unwrap();
        let store = Arc::new(SnapshotStore::new(RuntimeSnapshot::from_config(
            file_cfg.clone(),
        )));
        let effective = Arc::new(Mutex::new(EffectiveConfig::new(file_cfg.clone())));
        let state = ConfiguratorState {
            config_path: PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../../tests/fixtures/config/minimal.yaml"),
            base_dir: Some(
                PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/config"),
            ),
            metrics_hub: None,
            export_controller: None,
            events: None,
        };
        let handle = spawn(store.clone(), effective, state).handle();
        let gen0 = store.generation();

        let patch = Config {
            schema_version: 1,
            rules: Some(RulesConfig {
                match_mode: "first_match".into(),
                rules: vec![],
            }),
            ..Default::default()
        };
        let result = handle
            .apply_overlay(Some(patch), OverlayApplyMode::Merge, None)
            .await;
        assert!(!result.ok);
        assert!(result.errors.iter().any(|e| e.contains("rules")));
        assert_eq!(store.generation(), gen0);
    }

    #[tokio::test]
    async fn sparse_yaml_overlay_preserves_file_listeners() {
        use conduit_config::load_overlay_patch;

        let yaml = include_str!("../../../tests/fixtures/config/minimal.yaml");
        let file_cfg = load_yaml(yaml).unwrap();
        let file_listener_addr = file_cfg.listeners.as_ref().unwrap().listeners[0]
            .address
            .clone();
        let store = Arc::new(SnapshotStore::new(RuntimeSnapshot::from_config(
            file_cfg.clone(),
        )));
        let effective = Arc::new(Mutex::new(EffectiveConfig::new(file_cfg.clone())));
        let state = ConfiguratorState {
            config_path: PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../../tests/fixtures/config/minimal.yaml"),
            base_dir: Some(
                PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/config"),
            ),
            metrics_hub: None,
            export_controller: None,
            events: None,
        };
        let handle = spawn(store.clone(), effective, state).handle();

        let patch_yaml = r#"schema_version: 1
pools:
  - name: default
    backends:
      - address: "127.0.0.1:5300"
        weight: 50
"#;
        let patch = load_overlay_patch(patch_yaml).unwrap();
        let result = handle
            .apply_overlay(Some(patch), OverlayApplyMode::Merge, None)
            .await;
        assert!(result.ok, "{:?}", result.errors);
        assert_eq!(store.load().config.pools[0].backends[0].weight, Some(50));
        assert_eq!(
            store.load().config.listeners.as_ref().unwrap().listeners[0].address,
            file_listener_addr
        );
    }

    /// Regression: G4 `state_tx` must be dropped on shutdown or the select loop
    /// never exits (process hangs on SIGINT after export controller is wired).
    #[tokio::test]
    async fn shutdown_completes_after_export_controller_wired() {
        let yaml = include_str!("../../../tests/fixtures/config/minimal.yaml");
        let file_cfg = load_yaml(yaml).unwrap();
        let store = Arc::new(SnapshotStore::new(RuntimeSnapshot::from_config(
            file_cfg.clone(),
        )));
        let effective = Arc::new(Mutex::new(EffectiveConfig::new(file_cfg.clone())));
        let state = ConfiguratorState {
            config_path: PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../../tests/fixtures/config/minimal.yaml"),
            base_dir: Some(
                PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/config"),
            ),
            metrics_hub: None,
            export_controller: None,
            events: None,
        };
        let mut spawn = spawn(store, effective, state);

        let hub = Arc::new(conduit_metrics::MetricsHub::from_config(&file_cfg));
        let events = Arc::new(conduit_events::EventHub::disabled());
        let controller = Arc::new(conduit_metrics::MetricsExportController::new(hub));
        spawn.set_export_controller(controller, events);

        // Give the state update a moment to be processed.
        tokio::task::yield_now().await;

        tokio::time::timeout(std::time::Duration::from_secs(2), spawn.shutdown())
            .await
            .expect("configurator shutdown hung — state_tx likely still open");
    }

    #[tokio::test]
    async fn apply_accepts_collect_off_while_script_references_metric() {
        use conduit_proto::config::{MetricsConfig, UserMetricExportConfig};

        let yaml = include_str!("../../../tests/fixtures/config/metrics-consumer-blat-base.yaml");
        let file_cfg = load_yaml(yaml).unwrap();
        let base_dir =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/config");
        let store = Arc::new(SnapshotStore::new(RuntimeSnapshot::from_config_with_base(
            file_cfg.clone(),
            Some(&base_dir),
        )));
        let effective = Arc::new(Mutex::new(EffectiveConfig::new(file_cfg)));
        let state = ConfiguratorState {
            config_path: base_dir.join("metrics-consumer-blat-base.yaml"),
            base_dir: Some(base_dir),
            metrics_hub: None,
            export_controller: None,
            events: None,
        };
        let handle = spawn(store.clone(), effective, state).handle();
        let gen0 = store.generation();

        let patch = Config {
            schema_version: 1,
            metrics: Some(MetricsConfig {
                user_metrics: vec![UserMetricExportConfig {
                    name: "blat".into(),
                    export: String::new(),
                    collect: Some(false),
                    emit: Some(false),
                    help: String::new(),
                }],
                ..Default::default()
            }),
            ..Default::default()
        };
        let result = handle
            .apply_overlay(Some(patch), OverlayApplyMode::Merge, None)
            .await;
        assert!(
            result.ok,
            "collect-off write sites must apply; errors: {}",
            result.errors.join("\n")
        );
        assert!(store.generation() > gen0);
    }
}
