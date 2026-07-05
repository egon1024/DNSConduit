//! RequestRules hook — ordered built-in and Rhai rule actions.

use crate::build_routing_runtime_snapshot;
use crate::health::HealthRegistry;
use crate::phase::Phase;
use crate::pipeline::{PipelineStage, StageOutcome};
use crate::rules::{RuleHook, RuleOutcome};
use crate::snapshot::RuntimeSnapshot;
use crate::transaction::Transaction;
use conduit_metrics::MetricsHub;
use conduit_script::RoutingRuntimeSnapshot;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;

/// Snapshot in-flight upstream forwards per backend (from the dataplane txn table).
pub type OutstandingPerBackendFn = Arc<dyn Fn() -> HashMap<SocketAddr, u32> + Send + Sync>;

#[derive(Default)]
pub struct RequestRulesStage {
    pub metrics: Option<Arc<MetricsHub>>,
    pub health: Option<Arc<HealthRegistry>>,
    pub outstanding: Option<OutstandingPerBackendFn>,
}

impl RequestRulesStage {
    fn routing_runtime(&self, snapshot: &RuntimeSnapshot) -> Option<Arc<RoutingRuntimeSnapshot>> {
        let health = self.health.as_ref()?;
        let outstanding = self.outstanding.as_ref().map(|f| f()).unwrap_or_default();
        Some(Arc::new(build_routing_runtime_snapshot(
            &snapshot.config,
            &snapshot.health,
            health,
            &outstanding,
            snapshot.generation,
        )))
    }
}

impl PipelineStage for RequestRulesStage {
    fn name(&self) -> &'static str {
        "request_rules"
    }

    fn handle(&self, txn: &mut Transaction, snapshot: &Arc<RuntimeSnapshot>) -> StageOutcome {
        let routing_runtime = self.routing_runtime(snapshot);
        let result = snapshot.rules.eval(
            RuleHook::Request,
            txn,
            &snapshot.scripting,
            self.metrics.as_deref(),
            routing_runtime,
        );

        match result.outcome {
            RuleOutcome::Drop => StageOutcome::Drop,
            RuleOutcome::Continue | RuleOutcome::Retry => StageOutcome::Continue(Phase::Lookup),
        }
    }
}
