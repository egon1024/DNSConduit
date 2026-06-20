//! RequestRules hook — ordered built-in and Rhai rule actions.

use crate::phase::Phase;
use crate::pipeline::{PipelineStage, StageOutcome};
use crate::rules::{RuleHook, RuleOutcome};
use crate::snapshot::RuntimeSnapshot;
use crate::transaction::Transaction;
use conduit_metrics::MetricsHub;
use std::sync::Arc;

#[derive(Default)]
pub struct RequestRulesStage {
    pub metrics: Option<Arc<MetricsHub>>,
}

impl PipelineStage for RequestRulesStage {
    fn name(&self) -> &'static str {
        "request_rules"
    }

    fn handle(&self, txn: &mut Transaction, snapshot: &Arc<RuntimeSnapshot>) -> StageOutcome {
        let result = snapshot.rules.eval(
            RuleHook::Request,
            txn,
            &snapshot.scripting,
            self.metrics.as_deref(),
        );

        match result.outcome {
            RuleOutcome::Drop => StageOutcome::Drop,
            RuleOutcome::Continue | RuleOutcome::Retry => StageOutcome::Continue(Phase::Route),
        }
    }
}
