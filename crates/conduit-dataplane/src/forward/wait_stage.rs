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
    fn record_forward_outcome(&self, txn: &mut Transaction, snapshot: &RuntimeSnapshot) {
        let Some(hub) = self.metrics.as_ref() else {
            return;
        };
        if !hub.metrics_enabled() {
            return;
        }
        if txn.forward_metrics_recorded {
            return;
        }
        let pool = txn.selected_pool.as_deref().unwrap_or("unknown");
        let backend_label = txn
            .selected_backend
            .map(|addr| backend_metric_label_for_addr(&snapshot.config.pools, pool, addr))
            .unwrap_or_else(|| "unknown".into());
        let success = txn.response_wire.is_some();
        let outcome = if success { "success" } else { "error" };
        let builtin = txn.builtin_registry(hub);
        builtin.record_forward_attempt(pool, &backend_label, outcome);
        if !success {
            builtin.record_forward_error(pool, &backend_label, "timeout");
        }
        builtin.record_forward_duration(
            pool,
            &backend_label,
            txn.last_forward_ms() as f64 / 1000.0,
        );
        txn.forward_metrics_recorded = true;
        if let (Some(registry), Some(backend)) = (self.health.as_ref(), txn.selected_backend) {
            let is_failure = !success;
            if let Some(result) =
                registry.record_passive_forward_outcome(&snapshot.health, pool, backend, is_failure)
            {
                let qname = txn.qname.as_deref().unwrap_or("?");
                let qtype = txn.qtype.unwrap_or(0);
                if result.transitioned {
                    tracing::warn!(
                        %pool,
                        backend = %backend,
                        reason = "timeout",
                        %qname,
                        qtype,
                        client = %txn.client_addr,
                        passive_failures = result.consecutive_failures,
                        passive_fall = result.passive_fall,
                        "passive fast-trip: backend marked down"
                    );
                } else if result.already_down {
                    tracing::debug!(
                        %pool,
                        backend = %backend,
                        reason = "timeout",
                        %qname,
                        qtype,
                        client = %txn.client_addr,
                        "passive health: forward failure (backend already down)"
                    );
                } else {
                    tracing::warn!(
                        %pool,
                        backend = %backend,
                        reason = "timeout",
                        %qname,
                        qtype,
                        client = %txn.client_addr,
                        passive_failures = result.consecutive_failures,
                        passive_fall = result.passive_fall,
                        "passive health: forward failure"
                    );
                }
            }
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

#[cfg(test)]
mod tests {
    use super::*;
    use conduit_config::load_yaml;
    use conduit_core::snapshot::RuntimeSnapshot;
    use conduit_core::ClientProtocol;

    fn snapshot() -> Arc<RuntimeSnapshot> {
        let yaml = r#"
schema_version: 1
listeners:
  threads: 1
  listeners:
    - address: "127.0.0.1:15353"
      protocol: udp
metrics:
  enabled: true
  profile: full
pools:
  - name: default
    backends:
      - address: "127.0.0.1:15300"
        name: resolver-east
"#;
        Arc::new(RuntimeSnapshot::from_config(load_yaml(yaml).unwrap()))
    }

    fn metrics_hub() -> Arc<MetricsHub> {
        Arc::new(MetricsHub::from_config(&snapshot().config))
    }

    #[test]
    fn skips_forward_metrics_when_already_recorded() {
        let metrics = metrics_hub();
        let stage = WaitResponseStage::new(false, Some(metrics.clone()), None);
        let snap = snapshot();
        let mut txn = Transaction::new(1, "127.0.0.1:53".parse().unwrap(), ClientProtocol::Udp);
        txn.selected_pool = Some("default".into());
        txn.selected_backend = Some("127.0.0.1:15300".parse().unwrap());
        txn.selected_backend_label = Some("resolver-east".into());
        txn.response_wire = Some(vec![0u8; 12]);
        txn.forward_metrics_recorded = true;

        stage.handle(&mut txn, &snap);

        let body = conduit_metrics::render_prometheus(&metrics, &[]);
        assert!(
            !body.contains("conduit_forward_attempts_total"),
            "expected no forward attempt when already recorded, body:\n{body}"
        );
    }

    #[test]
    fn records_forward_metrics_when_not_yet_recorded() {
        let metrics = metrics_hub();
        let stage = WaitResponseStage::new(false, Some(metrics.clone()), None);
        let snap = snapshot();
        let mut txn = Transaction::new(2, "127.0.0.1:53".parse().unwrap(), ClientProtocol::Udp);
        txn.selected_pool = Some("default".into());
        txn.selected_backend = Some("127.0.0.1:15300".parse().unwrap());
        txn.selected_backend_label = Some("resolver-east".into());
        txn.response_wire = Some(vec![0u8; 12]);
        txn.last_forward_ms = 5;

        stage.handle(&mut txn, &snap);

        let body = conduit_metrics::render_prometheus(&metrics, &[]);
        assert!(
            body.contains(
                r#"conduit_forward_attempts_total{backend="resolver-east",outcome="success",pool="default"} 1"#
            ),
            "body:\n{body}"
        );
        assert!(txn.forward_metrics_recorded);
    }
}
