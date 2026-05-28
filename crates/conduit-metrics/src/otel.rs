//! OTEL metrics periodic push (background task).
//!
//! v1 ships a periodic tick that logs push attempts. Full OTLP instrument mapping from
//! Prometheus series is deferred; operators should use Prometheus scrape as the primary path.

use crate::MetricsHub;
use conduit_observation::ObservationHub;
use std::sync::Arc;
use std::time::Duration;

pub fn spawn_otel_push(
    endpoint: String,
    interval_ms: u32,
    resource_attributes: Vec<(String, String)>,
    hub: Arc<MetricsHub>,
    observation: Arc<ObservationHub>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let interval = Duration::from_millis(interval_ms.max(1000) as u64);
        loop {
            if hub.metrics_enabled() {
                let obs = observation.sink_metrics_snapshot();
                tracing::debug!(
                    endpoint = %endpoint,
                    sinks = obs.len(),
                    resource_attrs = resource_attributes.len(),
                    "otel metrics push tick (OTLP export not yet wired; use prometheus scrape)"
                );
            }
            tokio::time::sleep(interval).await;
        }
    })
}
