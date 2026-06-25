//! Sync runtime: blocking ingress workers with shared slot pool.

use crate::listener::supervisor;
use conduit_core::snapshot::SnapshotStore;
use conduit_metrics::{MetricsHub, TracingHub};
use std::io;
use std::sync::Arc;

pub use supervisor::DataplaneHandle;

pub fn start_sync(
    store: Arc<SnapshotStore>,
    metrics: Arc<MetricsHub>,
    tracing: Arc<TracingHub>,
) -> io::Result<DataplaneHandle> {
    supervisor::start(store, metrics, tracing)
}
