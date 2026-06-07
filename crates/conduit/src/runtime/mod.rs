//! Unified process lifecycle: start supervised services, wait for shutdown, stop cleanly.

use conduit_api::ControlHandle;
use conduit_config::{control_listen_addr, EffectiveConfig};
use conduit_core::configurator::ConfiguratorHandle;
use conduit_core::snapshot::SnapshotStore;
use conduit_dataplane::DataplaneHandle;
use conduit_metrics::{spawn_otel_push, spawn_prometheus_server, MetricsHub, TracingHub};
use conduit_proto::config::Config;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use tokio::task::JoinHandle;

pub struct RuntimeSupervisorArgs {
    pub store: Arc<SnapshotStore>,
    pub effective: Arc<Mutex<EffectiveConfig>>,
    pub configurator: ConfiguratorHandle,
    pub metrics_hub: Arc<MetricsHub>,
    pub tracing_hub: Arc<TracingHub>,
    pub file_cfg: Config,
}

pub struct RuntimeSupervisor {
    dataplane: DataplaneHandle,
    control: Option<ControlHandle>,
    prometheus: Option<JoinHandle<()>>,
    otel: Option<JoinHandle<()>>,
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
        } = args;

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
                endpoint.clone(),
                metrics_hub.compiled.otel_push_interval_ms,
                metrics_hub.compiled.otel_resource_attributes.clone(),
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
                     add a control section with listen_address to enable"
                );
                None
            }
            Some(addr) => {
                tracing::info!(%addr, "starting control plane");
                Some(conduit_api::spawn_control_plane(
                    addr,
                    store,
                    effective,
                    configurator,
                    tracing_hub,
                )?)
            }
        };

        Ok(Self {
            dataplane,
            control,
            prometheus,
            otel,
        })
    }

    pub async fn shutdown(self) {
        tracing::info!("stopping services");

        if let Some(control) = self.control {
            control.shutdown().await;
            tracing::debug!("control plane stopped");
        }

        if let Some(handle) = self.prometheus {
            handle.abort();
            tracing::debug!("prometheus metrics stopped");
        }

        if let Some(handle) = self.otel {
            handle.abort();
            tracing::debug!("otel metrics push stopped");
        }

        self.dataplane.shutdown();
        tracing::info!("services stopped");
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
