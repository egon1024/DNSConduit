//! Dataplane runtime factory (startup selection).

mod orchestrator;
mod split_io;
mod sync;

use conduit_config::effective_dataplane_runtime;
use conduit_core::snapshot::SnapshotStore;
use conduit_metrics::{MetricsHub, TracingHub};
use std::io;
use std::sync::Arc;

pub use sync::{start_sync, DataplaneHandle};

/// Start the dataplane for the configured runtime model.
pub fn start(
    store: Arc<SnapshotStore>,
    metrics: Arc<MetricsHub>,
    tracing: Arc<TracingHub>,
) -> io::Result<DataplaneHandle> {
    let snap = store.load();
    match effective_dataplane_runtime(&snap.config) {
        "sync" => start_sync(store, metrics, tracing),
        "split_io" => split_io::start_split_io(store, metrics, tracing),
        "tokio" => Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "dataplane.runtime tokio is not implemented yet",
        )),
        other => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("unknown dataplane.runtime '{other}'"),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use conduit_config::load_yaml;
    use conduit_core::RuntimeSnapshot;

    #[test]
    fn factory_starts_split_io_from_fixture() {
        let listen_port = {
            let s = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
            s.local_addr().unwrap().port()
        };
        let template = include_str!("../../../../tests/fixtures/config/dataplane-split-io.yaml");
        let yaml = template.replace("127.0.0.1:15353", &format!("127.0.0.1:{listen_port}"));
        let cfg = load_yaml(&yaml).unwrap();
        let store = Arc::new(SnapshotStore::new(RuntimeSnapshot::from_config(cfg)));
        let metrics = Arc::new(MetricsHub::from_config(&store.load().config));
        let tracing = Arc::new(TracingHub::from_config(&store.load().config));
        let handle = start(store, metrics, tracing).expect("split_io should start");
        handle.shutdown();
    }
}
