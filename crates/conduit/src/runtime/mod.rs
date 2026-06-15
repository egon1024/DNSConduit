//! Unified process lifecycle: start supervised services, wait for shutdown, stop cleanly.
//!
//! Future runtime work (not implemented here):
//! - Dynamic listener reconcile (resize/rebind workers without full process restart)
//! - Hot-start/stop control plane when `control:` changes via reload (restart required today)

use conduit_api::ControlHandle;
use conduit_config::{control_listen_addr, EffectiveConfig};
use conduit_core::configurator::{ConfiguratorHandle, ConfiguratorSpawn};
use conduit_core::snapshot::SnapshotStore;
use conduit_dataplane::DataplaneHandle;
use conduit_metrics::{
    spawn_otel_push, spawn_prometheus_server, MetricsHub, OtelPushHandle, OtelPushSettings,
    PrometheusServerHandle, TracingHub,
};
use conduit_proto::config::Config;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

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
    prometheus: Option<PrometheusServerHandle>,
    otel: Option<OtelPushHandle>,
    configurator: ConfiguratorSpawn,
    #[cfg(unix)]
    sighup: Option<SighupReloadTask>,
}

impl RuntimeSupervisor {
    pub async fn start(args: RuntimeSupervisorArgs) -> anyhow::Result<Self> {
        let RuntimeSupervisorArgs {
            store,
            effective,
            configurator,
            metrics_hub,
            tracing_hub,
            file_cfg,
            config_base_dir,
            #[cfg(unix)]
            sighup,
        } = args;

        let configurator_handle = configurator.handle();

        let dataplane =
            conduit_dataplane::start(store.clone(), metrics_hub.clone(), tracing_hub.clone())?;
        metrics_hub.set_scrape_snapshot_fn(conduit_dataplane::metrics_scrape::scrape_snapshot_fn(
            store.clone(),
            dataplane.txn_table.clone(),
        ));
        tracing::info!("dataplane listeners started");

        let prometheus = if let Some(ref addr) = metrics_hub.compiled.prometheus_listen {
            let listen: SocketAddr = addr.parse()?;
            Some(spawn_prometheus_server(
                listen,
                metrics_hub.compiled.prometheus_path.clone(),
                metrics_hub.clone(),
                dataplane.events.clone(),
            ))
        } else {
            None
        };

        let otel = if let Some(ref endpoint) = metrics_hub.compiled.otel_endpoint {
            Some(spawn_otel_push(
                OtelPushSettings {
                    endpoint: endpoint.clone(),
                    push_interval_ms: metrics_hub.compiled.otel_push_interval_ms,
                    resource_attributes: metrics_hub.compiled.otel_resource_attributes.clone(),
                    allow_invalid_certs: metrics_hub.compiled.otel_allow_invalid_certs,
                    headers: metrics_hub.compiled.otel_headers.clone(),
                },
                metrics_hub.clone(),
                dataplane.events.clone(),
            ))
        } else {
            None
        };

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
            prometheus,
            otel,
            configurator,
            #[cfg(unix)]
            sighup,
        })
    }

    pub async fn shutdown(self) {
        tracing::info!("stopping services");

        if let Some(control) = self.control {
            control.shutdown().await;
            tracing::debug!("control plane stopped");
        }

        if let Some(handle) = self.prometheus {
            handle.shutdown().await;
            tracing::debug!("prometheus metrics stopped");
        }

        if let Some(handle) = self.otel {
            handle.shutdown().await;
            tracing::debug!("otel metrics push stopped");
        }

        #[cfg(unix)]
        if let Some(task) = self.sighup {
            task.shutdown().await;
            tracing::debug!("sighup reload handler stopped");
        }

        self.configurator.shutdown().await;
        tracing::debug!("configurator stopped");

        self.dataplane.shutdown();
        tracing::info!("services stopped");
    }
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
