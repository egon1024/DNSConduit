//! Unified process lifecycle: start supervised services, wait for shutdown, stop cleanly.
//!
//! Future runtime work (not implemented here):
//! - Dynamic listener reconcile (resize/rebind workers without full process restart)
//! - Hot-start/stop control plane when `control:` changes via reload (restart required today)

use conduit_api::ControlHandle;
use conduit_config::{
    control_listen_addr, effective_drain, effective_drain_timeout_ms, EffectiveConfig,
};
use conduit_core::configurator::{ConfiguratorHandle, ConfiguratorSpawn};
use conduit_core::snapshot::SnapshotStore;
use conduit_dataplane::DataplaneHandle;
use conduit_metrics::{MetricsExportController, MetricsHub, TracingHub};
use conduit_proto::config::Config;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub struct RuntimeSupervisorArgs {
    pub store: Arc<SnapshotStore>,
    pub effective: Arc<Mutex<EffectiveConfig>>,
    pub configurator: ConfiguratorSpawn,
    pub metrics_hub: Arc<MetricsHub>,
    pub tracing_hub: Arc<TracingHub>,
    pub file_cfg: Config,
    pub config_base_dir: Option<PathBuf>,
    #[cfg(unix)]
    pub sighup: Option<SighupReloadTask>,
}

pub struct RuntimeSupervisor {
    dataplane: DataplaneHandle,
    control: Option<ControlHandle>,
    /// Export controller for hot-rebinding Prometheus/OTLP sinks (G4 hot-rebind).
    export_controller: Arc<MetricsExportController>,
    configurator: ConfiguratorSpawn,
    /// Live config snapshot, read at shutdown so drain settings (`shutdown.drain`,
    /// `shutdown.drain_timeout_ms`) reflect the latest applied/reloaded config
    /// rather than the values captured at process start.
    store: Arc<SnapshotStore>,
    #[cfg(unix)]
    sighup: Option<SighupReloadTask>,
}

impl RuntimeSupervisor {
    pub async fn start(args: RuntimeSupervisorArgs) -> anyhow::Result<Self> {
        let RuntimeSupervisorArgs {
            store,
            effective,
            mut configurator,
            metrics_hub,
            tracing_hub,
            file_cfg,
            config_base_dir,
            #[cfg(unix)]
            sighup,
        } = args;

        // Keep a handle to the live snapshot so shutdown can read drain settings
        // dynamically (the `store` below is moved into the control plane).
        let drain_store = store.clone();

        let dataplane =
            conduit_dataplane::start(store.clone(), metrics_hub.clone(), tracing_hub.clone())?;
        metrics_hub.set_scrape_snapshot_fn(conduit_dataplane::metrics_scrape::scrape_snapshot_fn(
            store.clone(),
            dataplane.txn_table.clone(),
            dataplane.txn_store.clone(),
        ));
        tracing::info!("dataplane listeners started");

        // Create export controller for hot-rebinding Prometheus/OTLP sinks.
        // Initial spawn uses the compiled config from startup.
        let export_controller = Arc::new(MetricsExportController::new(metrics_hub.clone()));
        let compiled = metrics_hub.compiled();
        export_controller
            .initial_spawn(&compiled, dataplane.events.clone())
            .await;

        // Wire export controller into configurator state for hot-rebind on apply.
        configurator.set_export_controller(export_controller.clone(), dataplane.events.clone());

        let configurator_handle = configurator.handle();

        let control = match control_listen_addr(&file_cfg)? {
            None => {
                tracing::info!(
                    "control plane disabled (no control section in config); \
                     conduitctl apply, export, reload, and trace are unavailable — \
                     add a control section with listen_address to enable (process restart required)"
                );
                None
            }
            Some(addr) => {
                tracing::info!(%addr, "starting control plane");
                Some(conduit_api::spawn_control_plane(
                    addr,
                    store,
                    effective,
                    configurator_handle,
                    tracing_hub,
                    config_base_dir,
                )?)
            }
        };

        Ok(Self {
            dataplane,
            control,
            export_controller,
            configurator,
            store: drain_store,
            #[cfg(unix)]
            sighup,
        })
    }

    /// Stop all services and drain in-flight transactions before tearing down
    /// listeners.
    ///
    /// Drain settings are resolved from the **live** config snapshot at the
    /// moment shutdown begins, so a `conduitctl apply` or reload that changed
    /// `shutdown.drain` / `shutdown.drain_timeout_ms` takes effect on the next
    /// shutdown without a process restart.
    ///
    /// `force_exit` is shared with a background task watching for a second
    /// `SIGINT`/`SIGTERM`; when it flips to `true` the drain wait is abandoned
    /// and the process proceeds straight to listener teardown. Draining is
    /// skipped entirely when disabled via `shutdown.drain: false`.
    pub async fn shutdown(self, force_exit: Arc<AtomicBool>) {
        tracing::info!("stopping services");

        if let Some(control) = self.control {
            control.shutdown().await;
            tracing::debug!("control plane stopped");
        }

        // Shut down export sinks (Prometheus/OTLP) via the controller.
        self.export_controller.shutdown().await;
        tracing::debug!("metrics export sinks stopped");

        #[cfg(unix)]
        if let Some(task) = self.sighup {
            task.shutdown().await;
            tracing::debug!("sighup reload handler stopped");
        }

        self.configurator.shutdown().await;
        tracing::debug!("configurator stopped");

        // Best-effort graceful drain of in-flight transaction slots before the
        // abrupt listener teardown (preparation for zero-downtime-upgrade).
        // Resolve drain behavior from the live snapshot so an applied/reloaded
        // `shutdown:` change takes effect here without a restart.
        let (drain_enabled, drain_timeout) = drain_settings(&self.store);
        if drain_enabled {
            match self.dataplane.drain(drain_timeout, None, Some(&force_exit)) {
                conduit_dataplane::DrainOutcome::Drained => {
                    tracing::debug!("dataplane slots drained");
                }
                conduit_dataplane::DrainOutcome::TimedOut { remaining } => {
                    tracing::warn!(
                        remaining,
                        timeout_ms = drain_timeout.as_millis() as u64,
                        "dataplane drain timed out; forcing shutdown"
                    );
                }
                conduit_dataplane::DrainOutcome::Aborted { remaining } => {
                    tracing::warn!(
                        remaining,
                        "second shutdown signal received; abandoning drain and exiting"
                    );
                }
            }
        } else {
            tracing::debug!("drain disabled (shutdown.drain: false); skipping graceful drain");
        }

        self.dataplane.shutdown();
        tracing::info!("services stopped");
    }
}

/// Resolve the drain behavior (`shutdown.drain`, `shutdown.drain_timeout_ms`)
/// from the current snapshot. Reading the live store here — rather than caching
/// values at process start — is what makes the `shutdown:` block hot: an
/// `apply`/reload swaps a new snapshot in, and the next shutdown observes it.
fn drain_settings(store: &SnapshotStore) -> (bool, Duration) {
    let snap = store.load();
    let enabled = effective_drain(&snap.config);
    let timeout = Duration::from_millis(effective_drain_timeout_ms(&snap.config) as u64);
    (enabled, timeout)
}

/// Spawn a background task that flips `force_exit` to `true` on the next
/// `SIGINT`/`SIGTERM`, letting an in-progress drain abandon its wait. Call this
/// after the first shutdown signal has already been received.
pub fn spawn_force_exit_listener(force_exit: Arc<AtomicBool>) {
    tokio::spawn(async move {
        if wait_for_shutdown_signal().await.is_ok() {
            force_exit.store(true, Ordering::SeqCst);
            tracing::warn!("second shutdown signal received; abandoning drain");
        }
    });
}

/// Background SIGHUP → config reload handler (Unix only).
#[cfg(unix)]
pub struct SighupReloadTask {
    shutdown_tx: tokio::sync::watch::Sender<()>,
    join: tokio::task::JoinHandle<()>,
}

#[cfg(unix)]
impl SighupReloadTask {
    pub fn spawn(configurator: ConfiguratorHandle) -> Self {
        let (shutdown_tx, mut shutdown_rx) = tokio::sync::watch::channel(());
        let join = tokio::spawn(async move {
            use conduit_core::ProposalSource;
            use tokio::signal::unix::{signal, SignalKind};

            let mut sighup = match signal(SignalKind::hangup()) {
                Ok(s) => s,
                Err(e) => {
                    tracing::warn!("SIGHUP handler not installed: {e}");
                    return;
                }
            };
            loop {
                tokio::select! {
                    changed = shutdown_rx.changed() => {
                        if changed.is_ok() {
                            break;
                        }
                    }
                    msg = sighup.recv() => {
                        if msg.is_none() {
                            break;
                        }
                        let result = configurator.reload_from_file(ProposalSource::Sighup).await;
                        if !result.ok {
                            tracing::error!(
                                errors = %result.errors.join("; "),
                                "SIGHUP config reload failed"
                            );
                        }
                    }
                }
            }
        });
        Self { shutdown_tx, join }
    }

    pub async fn shutdown(self) {
        let _ = self.shutdown_tx.send(());
        match self.join.await {
            Ok(()) => {}
            Err(e) if e.is_panic() => std::panic::resume_unwind(e.into_panic()),
            Err(e) => tracing::warn!(error = %e, "sighup reload task failed"),
        }
    }
}

/// Wait for process shutdown (SIGINT/SIGTERM on Unix, Ctrl+C elsewhere). SIGHUP is not included.
pub async fn wait_for_shutdown_signal() -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{signal, SignalKind};
        let mut sigterm = signal(SignalKind::terminate())?;
        let mut sigint = signal(SignalKind::interrupt())?;
        tokio::select! {
            _ = sigterm.recv() => {}
            _ = sigint.recv() => {}
        }
        Ok(())
    }
    #[cfg(not(unix))]
    {
        tokio::signal::ctrl_c().await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use conduit_core::RuntimeSnapshot;
    use conduit_proto::config::ShutdownConfig;

    /// Minimal config that compiles into a snapshot without panicking.
    const BASE_YAML: &str = r#"
schema_version: 1
listeners:
  threads: 1
  listeners:
    - address: "127.0.0.1:0"
      protocol: udp
forward:
  outstanding_per_backend: 10
  timeout_ms: 1000
orchestrator:
  max_attempts: 1
  max_txn_duration_ms: 1000
  txn_table_capacity: 64
pools:
  - name: default
    backends:
      - address: "127.0.0.1:5300"
        weight: 100
"#;

    fn config_with_shutdown(shutdown: Option<ShutdownConfig>) -> Config {
        let mut cfg = conduit_config::load_yaml(BASE_YAML).expect("base yaml parses");
        cfg.shutdown = shutdown;
        cfg
    }

    fn store_with(shutdown: Option<ShutdownConfig>) -> SnapshotStore {
        SnapshotStore::new(RuntimeSnapshot::from_config(config_with_shutdown(shutdown)))
    }

    #[test]
    fn drain_settings_use_defaults_when_block_absent() {
        let store = store_with(None);
        let (enabled, timeout) = drain_settings(&store);
        assert!(enabled);
        assert_eq!(timeout, Duration::from_millis(5000));
    }

    #[test]
    fn drain_settings_reflect_explicit_values() {
        let store = store_with(Some(ShutdownConfig {
            drain: Some(false),
            drain_timeout_ms: Some(250),
        }));
        let (enabled, timeout) = drain_settings(&store);
        assert!(!enabled);
        assert_eq!(timeout, Duration::from_millis(250));
    }

    #[test]
    fn drain_settings_track_live_snapshot_swaps() {
        // Start with a 1s drain...
        let store = store_with(Some(ShutdownConfig {
            drain: Some(true),
            drain_timeout_ms: Some(1000),
        }));
        let (enabled, timeout) = drain_settings(&store);
        assert!(enabled);
        assert_eq!(timeout, Duration::from_millis(1000));

        // ...then simulate `conduitctl apply`/reload swapping a new snapshot in.
        store.swap(RuntimeSnapshot::from_config(config_with_shutdown(Some(
            ShutdownConfig {
                drain: Some(false),
                drain_timeout_ms: Some(250),
            },
        ))));

        // The resolver reads the live snapshot, so the new values win without a restart.
        let (enabled, timeout) = drain_settings(&store);
        assert!(!enabled);
        assert_eq!(timeout, Duration::from_millis(250));
    }
}
