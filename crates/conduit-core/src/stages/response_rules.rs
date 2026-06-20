//! ResponseRules hook — ordered built-in and Rhai rule actions, and retry intent.

use crate::phase::Phase;
use crate::pipeline::{PipelineStage, StageOutcome};
use crate::rules::{RuleHook, RuleOutcome};
use crate::snapshot::RuntimeSnapshot;
use crate::transaction::Transaction;
use conduit_metrics::MetricsHub;
use std::sync::Arc;

#[derive(Default)]
pub struct ResponseRulesStage {
    pub metrics: Option<Arc<MetricsHub>>,
}

impl PipelineStage for ResponseRulesStage {
    fn name(&self) -> &'static str {
        "response_rules"
    }

    fn handle(&self, txn: &mut Transaction, snapshot: &Arc<RuntimeSnapshot>) -> StageOutcome {
        let result = snapshot.rules.eval(
            RuleHook::Response,
            txn,
            &snapshot.scripting,
            self.metrics.as_deref(),
        );

        if result.outcome == RuleOutcome::Retry {
            return StageOutcome::Continue(Phase::Route);
        }

        match result.outcome {
            RuleOutcome::Drop => StageOutcome::Drop,
            RuleOutcome::Continue | RuleOutcome::Retry => StageOutcome::Continue(Phase::Send),
        }
    }
}
