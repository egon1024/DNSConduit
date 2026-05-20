use conduit_api::serve;
use conduit_config::{load_yaml, validate, EffectiveConfig};
use conduit_core::{RuntimeSnapshot, SnapshotStore};
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();

    let path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "conduit.yaml".into());
    let yaml = std::fs::read_to_string(&path)?;
    let file_cfg = load_yaml(&yaml)?;
    let validation = validate(&file_cfg);
    if !validation.ok {
        anyhow::bail!("config invalid: {:?}", validation.errors);
    }

    let store = Arc::new(SnapshotStore::new(RuntimeSnapshot::from_config(
        file_cfg.clone(),
    )));
    let effective = Arc::new(Mutex::new(EffectiveConfig::new(file_cfg)));

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
