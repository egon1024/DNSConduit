//! ResponseRules hook — ordered built-in and Rhai rule actions, and retry intent.

use crate::build_routing_runtime_snapshot;
use crate::health::HealthRegistry;
use crate::phase::Phase;
use crate::pipeline::{PipelineStage, StageOutcome};
use crate::rules::{RuleHook, RuleOutcome};
use crate::snapshot::RuntimeSnapshot;
use crate::stages::request_rules::OutstandingPerBackendFn;
use crate::transaction::Transaction;
use conduit_metrics::MetricsHub;
use conduit_script::RoutingRuntimeSnapshot;
use std::sync::Arc;

#[derive(Default)]
pub struct ResponseRulesStage {
    pub metrics: Option<Arc<MetricsHub>>,
    pub health: Option<Arc<HealthRegistry>>,
    pub outstanding: Option<OutstandingPerBackendFn>,
}

impl ResponseRulesStage {
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

impl PipelineStage for ResponseRulesStage {
    fn name(&self) -> &'static str {
        "response_rules"
    }

    fn handle(&self, txn: &mut Transaction, snapshot: &Arc<RuntimeSnapshot>) -> StageOutcome {
        let routing_runtime = self.routing_runtime(snapshot);
        let result = snapshot.rules.eval(
            RuleHook::Response,
            txn,
            &snapshot.scripting,
            self.metrics.as_deref(),
            routing_runtime,
        );

        if result.outcome == RuleOutcome::Retry {
            if txn.response_wire.is_none() && txn.convergence_reason.is_some() {
                // Route never selected a backend; bump so Route honors stashed retry_pool.
                txn.attempt_count = txn.attempt_count.max(1);
            }
            return StageOutcome::Continue(Phase::Lookup);
        }

        let answerless_route_failure =
            txn.response_wire.is_none() && txn.convergence_reason.is_some();

        match result.outcome {
            RuleOutcome::Drop => StageOutcome::Drop,
            RuleOutcome::Continue | RuleOutcome::Retry => {
                if answerless_route_failure {
                    StageOutcome::Continue(Phase::NoAnswer)
                } else {
                    StageOutcome::Continue(Phase::Send)
                }
            }
        }
    }
}
