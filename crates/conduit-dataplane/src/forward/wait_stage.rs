//! WaitResponse checkpoint after upstream I/O completes.

use conduit_core::health::HealthRegistry;
use conduit_core::phase::Phase;
use conduit_core::pipeline::{PipelineStage, StageOutcome};
use conduit_core::record_upstream_response;
use conduit_core::routing::backend_metric_label_for_addr;
use conduit_core::snapshot::RuntimeSnapshot;
use conduit_core::Transaction;
use conduit_metrics::MetricsHub;
use std::sync::Arc;

/// Resume checkpoint: upstream reply or timeout is already on the transaction.
///
/// This stage runs only in the `split_io` runtime, where the upstream forward is
/// sent on a policy worker and completed asynchronously by the I/O backend. It is
/// therefore the point where `split_io` records the forward attempt metrics that
/// the synchronous runtime records inline in [`crate::forward::ForwardTransport`].
pub struct WaitResponseStage {
    parse_wire_meta: bool,
    metrics: Option<Arc<MetricsHub>>,
    health: Option<Arc<HealthRegistry>>,
}

impl WaitResponseStage {
    pub fn new(
        parse_wire_meta: bool,
        metrics: Option<Arc<MetricsHub>>,
        health: Option<Arc<HealthRegistry>>,
    ) -> Self {
        Self {
            parse_wire_meta,
            metrics,
            health,
        }
    }

    /// Record `conduit_forward_*` for the just-completed upstream wait leg.
    ///
    /// A present `response_wire` means the upstream replied (success); its absence
    /// means the I/O backend timed the forward out. Labels resolve the backend
    /// `name` (when configured) via the selected pool, matching the synchronous
    /// runtime so successes and timeouts for one backend share a label set.
    fn record_forward_outcome(&self, txn: &Transaction, snapshot: &RuntimeSnapshot) {
        let Some(hub) = self.metrics.as_ref() else {
            return;
        };
        if !hub.metrics_enabled() {
            return;
        }
        let pool = txn.selected_pool.as_deref().unwrap_or("unknown");
        let backend_label = txn
            .selected_backend
            .map(|addr| backend_metric_label_for_addr(&snapshot.config.pools, pool, addr))
            .unwrap_or_else(|| "unknown".into());
        let success = txn.response_wire.is_some();
        let outcome = if success { "success" } else { "error" };
        hub.builtin
            .record_forward_attempt(pool, &backend_label, outcome);
        if !success {
            hub.builtin.record_forward_error(pool, "timeout");
        }
        hub.builtin.record_forward_duration(
            pool,
            &backend_label,
            txn.last_forward_ms() as f64 / 1000.0,
        );
        if let (Some(registry), Some(backend)) = (self.health.as_ref(), txn.selected_backend) {
            registry.record_passive_forward_outcome(&snapshot.health, pool, backend, !success);
        }
    }
}

impl PipelineStage for WaitResponseStage {
    fn name(&self) -> &'static str {
        "wait_response"
    }

    fn handle(&self, txn: &mut Transaction, snapshot: &Arc<RuntimeSnapshot>) -> StageOutcome {
        self.record_forward_outcome(txn, snapshot);
        if let Some(wire) = txn.response_wire.clone() {
            record_upstream_response(txn, &wire, self.parse_wire_meta);
        }
        StageOutcome::Continue(Phase::ResponseRules)
    }
}
