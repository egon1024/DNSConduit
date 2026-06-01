//! Serialized configuration apply path (phase 5).
//!
//! All snapshot installs flow through the Configurator so phase 5b can enqueue Rhai proposals
//! on the same channel without refactoring gRPC handlers.

use crate::snapshot::{RuntimeSnapshot, SnapshotStore};
use conduit_config::{clear_overlay, load_yaml, validate, EffectiveConfig, ValidationResult};
use conduit_proto::config::Config;
use std::fmt;
use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tokio::sync::{mpsc, oneshot};

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

/// Configuration change submitted to the Configurator.
#[derive(Debug, Clone)]
pub struct PolicyProposal {
    pub source: ProposalSource,
    /// gRPC overlay patch (replaces API overlay layer).
    pub overlay: Option<Config>,
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
        overlay: Config,
        correlation_id: Option<String>,
    ) -> ApplyResult {
        self.propose(PolicyProposal {
            source: ProposalSource::Grpc,
            overlay: Some(overlay),
            file_reload: None,
            correlation_id,
        })
        .await
    }

    pub async fn reload_from_file(&self, source: ProposalSource) -> ApplyResult {
        self.propose(PolicyProposal {
            source,
            overlay: None,
            file_reload: None,
            correlation_id: None,
        })
        .await
    }
}

/// Spawn the Configurator consumer task. Returns a handle for enqueueing proposals.
pub fn spawn(
    store: Arc<SnapshotStore>,
    effective: Arc<Mutex<EffectiveConfig>>,
    state: ConfiguratorState,
) -> ConfiguratorHandle {
    let (tx, mut rx) = mpsc::channel::<ProposalEnvelope>(64);
    let handle = ConfiguratorHandle { tx: tx.clone() };

    tokio::spawn(async move {
        while let Some(envelope) = rx.recv().await {
            let result = apply_proposal(&store, &effective, &state, envelope.proposal).await;
            let _ = envelope.reply.send(result);
        }
    });

    handle
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

    if let Some(overlay) = &proposal.overlay {
        eff.overlay = Some(overlay.clone());
    }

    let merged = eff.effective();
    let ValidationResult { ok, errors } = validate(&merged);
    if !ok {
        return Err(errors);
    }
    Ok(merged)
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
        let handle = spawn(store.clone(), effective, state);

        let mut overlay = file_cfg.clone();
        overlay.pools[0].backends[0].weight = Some(42);

        let result = handle.apply_overlay(overlay, None).await;
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
        let handle = spawn(store.clone(), effective, state);
        let gen0 = store.generation();

        let mut overlay = file_cfg.clone();
        overlay.listeners.as_mut().unwrap().threads = 0;

        let result = handle.apply_overlay(overlay, None).await;
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
        let handle = spawn(store.clone(), effective.clone(), state);

        let mut overlay = file_cfg.clone();
        overlay.pools[0].backends[0].weight = Some(42);
        handle.apply_overlay(overlay, None).await;
        assert_eq!(store.load().config.pools[0].backends[0].weight, Some(42));

        let result = handle.reload_from_file(ProposalSource::Sighup).await;
        assert!(result.ok, "{:?}", result.errors);
        assert_eq!(store.load().config.pools[0].backends[0].weight, Some(100));
        let eff = effective.lock().unwrap();
        assert!(eff.overlay.is_none());
    }
}
