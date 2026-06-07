//! Background metrics task handles with cooperative shutdown.

use tokio::task::JoinHandle;

/// Handle for a spawned Prometheus HTTP scrape server.
pub struct PrometheusServerHandle {
    shutdown_tx: tokio::sync::oneshot::Sender<()>,
    join: JoinHandle<()>,
}

impl PrometheusServerHandle {
    pub(crate) fn new(shutdown_tx: tokio::sync::oneshot::Sender<()>, join: JoinHandle<()>) -> Self {
        Self { shutdown_tx, join }
    }

    pub async fn shutdown(self) {
        let _ = self.shutdown_tx.send(());
        match self.join.await {
            Ok(()) => {}
            Err(e) if e.is_panic() => std::panic::resume_unwind(e.into_panic()),
            Err(e) => tracing::warn!(error = %e, "prometheus metrics task failed"),
        }
    }
}

/// Handle for a spawned OTLP metrics push loop.
pub struct OtelPushHandle {
    shutdown_tx: tokio::sync::oneshot::Sender<()>,
    join: JoinHandle<()>,
}

impl OtelPushHandle {
    pub(crate) fn new(shutdown_tx: tokio::sync::oneshot::Sender<()>, join: JoinHandle<()>) -> Self {
        Self { shutdown_tx, join }
    }

    pub async fn shutdown(self) {
        let _ = self.shutdown_tx.send(());
        match self.join.await {
            Ok(()) => {}
            Err(e) if e.is_panic() => std::panic::resume_unwind(e.into_panic()),
            Err(e) => tracing::warn!(error = %e, "otel metrics push task failed"),
        }
    }
}
