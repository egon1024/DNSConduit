use conduit_api::serve;
use conduit_config::{init_from_config, load_yaml, validate, EffectiveConfig};
use conduit_core::{RuntimeSnapshot, SnapshotStore};
use std::net::SocketAddr;
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

    let mut snapshot =
        RuntimeSnapshot::from_config_with_base(file_cfg.clone(), base_dir.as_deref());
    let store = Arc::new(SnapshotStore::new(snapshot.clone()));
    snapshot.generation = store.generation();
    store.swap(snapshot);
    let effective = Arc::new(Mutex::new(EffectiveConfig::new(file_cfg)));

    let _dataplane = conduit_dataplane::supervisor::start(store.clone())?;
    tracing::info!("dataplane listeners started");

    let listen_address = store
        .load()
        .config
        .control
        .as_ref()
        .map(|c| c.listen_address.clone())
        .unwrap_or_else(|| "127.0.0.1:5199".into());
    let addr: SocketAddr = listen_address.parse()?;

    tracing::info!(%addr, "starting control plane");
    serve(addr, store, effective).await?;
    Ok(())
}
