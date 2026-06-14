//! Serialized configuration apply path (phase 5).
//!
//! All snapshot installs flow through the Configurator so phase 5b can enqueue Rhai proposals
//! on the same channel without refactoring gRPC handlers.

use crate::snapshot::{RuntimeSnapshot, SnapshotStore};
use conduit_config::{
    clear_overlay, is_overlay_patch_empty, load_yaml, merge_overlay_patches, validate,
    EffectiveConfig, ValidationResult,
};
use conduit_proto::config::Config;
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

/// Configuration change submitted to the Configurator.
#[derive(Debug, Clone)]
pub struct PolicyProposal {
    pub source: ProposalSource,
    /// gRPC overlay patch (interpretation depends on [`Self::overlay_mode`]).
    pub overlay: Option<Config>,
    pub overlay_mode: OverlayApplyMode,
    /// When set, replaces the file baseline (reload from disk).
    pub file_reload: Option<Config>,
    pub correlation_id: Option<String>,
}

/// Outcome of a single apply attempt.
#[derive(Debug, Clone)]
pub struct ApplyResult {
    pub ok: bool,
    pub errors: Vec<String>,
    pub generation: u64,
}

struct ProposalEnvelope {
    proposal: PolicyProposal,
    reply: oneshot::Sender<ApplyResult>,
}

/// Shared state for the Configurator loop.
pub struct ConfiguratorState {
    pub config_path: PathBuf,
    pub base_dir: Option<PathBuf>,
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
            };
        }
        reply_rx.await.unwrap_or(ApplyResult {
            ok: false,
            errors: vec!["configurator dropped reply".into()],
            generation: 0,
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
        })
        .await
    }
}

/// Spawn product of the Configurator background task.
pub struct ConfiguratorSpawn {
    handle: ConfiguratorHandle,
    task: JoinHandle<()>,
}

impl ConfiguratorSpawn {
    pub fn handle(&self) -> ConfiguratorHandle {
        self.handle.clone()
    }

    pub async fn shutdown(self) {
        drop(self.handle);
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
    let handle = ConfiguratorHandle { tx: tx.clone() };

    let task = tokio::spawn(async move {
        while let Some(envelope) = rx.recv().await {
            let result = apply_proposal(&store, &effective, &state, envelope.proposal).await;
            let _ = envelope.reply.send(result);
        }
    });

    ConfiguratorSpawn { handle, task }
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
            };
        }
    };

    match store.install_validated_with_base(merged, state.base_dir.as_deref()) {
        Ok(()) => {
            let new = store.load();
            log_config_applied(proposal.source, new.generation, &prev, &new);
            ApplyResult {
                ok: true,
                errors: vec![],
                generation: new.generation,
            }
        }
        Err(errors) => ApplyResult {
            ok: false,
            errors,
            generation: store.generation(),
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

    if let Some(file_cfg) = &proposal.file_reload {
        eff.file = file_cfg.clone();
        clear_overlay(&mut eff);
    } else if matches!(
        proposal.source,
        ProposalSource::File | ProposalSource::Sighup
    ) && proposal.overlay.is_none()
    {
        let yaml = fs::read_to_string(&state.config_path)
            .map_err(|e| vec![format!("reading config {:?}: {e}", state.config_path)])?;
        let file_cfg = load_yaml(&yaml).map_err(|e| vec![e.to_string()])?;
        eff.file = file_cfg;
        clear_overlay(&mut eff);
    }

    if proposal.source == ProposalSource::Grpc {
        apply_overlay_mode(&mut eff, proposal)?;
    }

    let merged = eff.effective();
    let ValidationResult { ok, errors } = validate(&merged);
    if !ok {
        return Err(errors);
    }
    Ok(merged)
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
            if is_overlay_patch_empty(patch) {
                clear_overlay(eff);
            } else {
                eff.overlay = Some(patch.clone());
            }
            Ok(())
        }
        OverlayApplyMode::Merge => {
            let Some(patch) = &proposal.overlay else {
                return Err(vec!["merge apply requires overlay".into()]);
            };
            if is_overlay_patch_empty(patch) {
                return Ok(());
            }
            eff.overlay = Some(match &eff.overlay {
                Some(existing) => merge_overlay_patches(existing, patch),
                None => patch.clone(),
            });
            Ok(())
        }
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
    log_pending_reconcile(&prev.config, &new.config);
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

fn log_pending_reconcile(prev: &Config, new: &Config) {
    let listeners_changed = prev.listeners != new.listeners;
    let forward_changed = prev.forward != new.forward;
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
        };
        let handle = spawn(store.clone(), effective, state).handle();

        let mut overlay = file_cfg.clone();
        overlay.pools[0].backends[0].weight = Some(42);

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
        };
        let handle = spawn(store.clone(), effective, state).handle();
        let gen0 = store.generation();

        let mut overlay = file_cfg.clone();
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
        };
        let handle = spawn(store.clone(), effective.clone(), state).handle();

        let mut overlay = file_cfg.clone();
        overlay.pools[0].backends[0].weight = Some(42);
        handle
            .apply_overlay(Some(overlay), OverlayApplyMode::Merge, None)
            .await;
        assert_eq!(store.load().config.pools[0].backends[0].weight, Some(42));

        let result = handle.reload_from_file(ProposalSource::Sighup).await;
        assert!(result.ok, "{:?}", result.errors);
        assert_eq!(store.load().config.pools[0].backends[0].weight, Some(100));
        let eff = effective.lock().unwrap();
        assert!(eff.overlay.is_none());
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
}
