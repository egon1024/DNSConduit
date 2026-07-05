mod runtime;

use conduit_config::{init_from_config, load_yaml, validate, EffectiveConfig};
use conduit_core::{spawn_configurator, ConfiguratorState, RuntimeSnapshot, SnapshotStore};
use conduit_metrics::{MetricsHub, TracingHub};
#[cfg(unix)]
use runtime::SighupReloadTask;
use runtime::{
    spawn_force_exit_listener, wait_for_shutdown_signal, RuntimeSupervisor, RuntimeSupervisorArgs,
};
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "conduit.yaml".into());
    let yaml = std::fs::read_to_string(&path).map_err(|e| {
        if e.to_string().contains("UTF-8") {
            anyhow::anyhow!(
                "reading config {:?}: {e} (pass the YAML path only, e.g. tests/manual/config/ipv4-ipv6-forwarding-v4-only.yml — not the conduit binary)",
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
        RuntimeSnapshot::try_from_config_with_base(file_cfg.clone(), base_dir.as_deref()).map_err(
            |e| {
                eprintln!("{e}");
                anyhow::anyhow!("config compile failed")
            },
        )?;
    let store = Arc::new(SnapshotStore::new(snapshot.clone()));
    snapshot.generation = store.generation();
    store.swap(snapshot);
    let effective = Arc::new(Mutex::new(EffectiveConfig::new(file_cfg.clone())));

    let configurator_state = ConfiguratorState {
        config_path: PathBuf::from(&path),
        base_dir: base_dir.clone(),
    };
    let configurator = spawn_configurator(store.clone(), effective.clone(), configurator_state);

    #[cfg(unix)]
    let sighup = Some(SighupReloadTask::spawn(configurator.handle()));

    let supervisor = RuntimeSupervisor::start(RuntimeSupervisorArgs {
        store,
        effective,
        configurator,
        metrics_hub,
        tracing_hub,
        file_cfg,
        config_base_dir: base_dir,
        #[cfg(unix)]
        sighup,
    })
    .await?;

    wait_for_shutdown_signal().await?;
    tracing::info!("shutdown signal received");

    // Watch for a second signal so an operator can abandon the drain wait and
    // exit immediately instead of waiting out shutdown.drain_timeout_ms.
    let force_exit = Arc::new(AtomicBool::new(false));
    spawn_force_exit_listener(force_exit.clone());

    supervisor.shutdown(force_exit).await;
    Ok(())
}
