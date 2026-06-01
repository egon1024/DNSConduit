use conduit_api::serve;
use conduit_config::{init_from_config, load_yaml, validate, EffectiveConfig};
use conduit_core::{
    spawn_configurator, ConfiguratorState, ProposalSource, RuntimeSnapshot, SnapshotStore,
};
use conduit_metrics::{spawn_otel_push, spawn_prometheus_server, MetricsHub, TracingHub};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "conduit.yaml".into());
    let yaml = std::fs::read_to_string(&path).map_err(|e| {
        if e.to_string().contains("UTF-8") {
            anyhow::anyhow!(
                "reading config {:?}: {e} (pass the YAML path only, e.g. tests/manual/config/01-v4-only.yaml — not the conduit binary)",
                path
            )
        } else {
            anyhow::Error::from(e).context(format!("reading config {path:?}"))
        }
    })?;
    let base_dir = std::path::Path::new(&path)
        .parent()
        .map(|p| p.to_path_buf());
    let file_cfg = load_yaml(&yaml)?;
    let validation = validate(&file_cfg);
    if !validation.ok {
        eprintln!("config invalid: {:?}", validation.errors);
        anyhow::bail!("config invalid");
    }

    init_from_config(file_cfg.logging.as_ref())?;

    let metrics_hub = Arc::new(MetricsHub::from_config(&file_cfg));
    let tracing_hub = Arc::new(TracingHub::from_config(&file_cfg));

    let mut snapshot =
        RuntimeSnapshot::from_config_with_base(file_cfg.clone(), base_dir.as_deref());
    let store = Arc::new(SnapshotStore::new(snapshot.clone()));
    snapshot.generation = store.generation();
    store.swap(snapshot);
    let effective = Arc::new(Mutex::new(EffectiveConfig::new(file_cfg.clone())));

    let configurator_state = ConfiguratorState {
        config_path: PathBuf::from(&path),
        base_dir,
    };
    let configurator = spawn_configurator(store.clone(), effective.clone(), configurator_state);

    #[cfg(unix)]
    spawn_sighup_handler(configurator.clone());

    let dataplane = conduit_dataplane::supervisor::start(
        store.clone(),
        metrics_hub.clone(),
        tracing_hub.clone(),
    )?;
    metrics_hub.set_scrape_snapshot_fn(conduit_dataplane::metrics_scrape::scrape_snapshot_fn(
        store.clone(),
        dataplane.txn_table.clone(),
    ));
    tracing::info!("dataplane listeners started");

    let mut _prometheus = if let Some(ref addr) = metrics_hub.compiled.prometheus_listen {
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

    let mut _otel = if let Some(ref endpoint) = metrics_hub.compiled.otel_endpoint {
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

    let listen_address = store
        .load()
        .config
        .control
        .as_ref()
        .map(|c| c.listen_address.clone())
        .unwrap_or_else(|| "127.0.0.1:5199".into());
    let addr: SocketAddr = listen_address.parse()?;

    tracing::info!(%addr, "starting control plane");
    serve(addr, store, effective, configurator, tracing_hub).await?;
    Ok(())
}

#[cfg(unix)]
fn spawn_sighup_handler(configurator: conduit_core::ConfiguratorHandle) {
    tokio::spawn(async move {
        use tokio::signal::unix::{signal, SignalKind};
        let mut sighup = match signal(SignalKind::hangup()) {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!("SIGHUP handler not installed: {e}");
                return;
            }
        };
        while sighup.recv().await.is_some() {
            let result = configurator.reload_from_file(ProposalSource::Sighup).await;
            if !result.ok {
                tracing::error!(errors = ?result.errors, "SIGHUP config reload failed");
            }
        }
    });
}
