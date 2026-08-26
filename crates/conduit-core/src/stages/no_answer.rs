//! NoAnswer phase — total-failure convergence before Send.

use crate::phase::Phase;
use crate::pipeline::{PipelineStage, StageOutcome};
use crate::rules::{RuleHook, RuleOutcome};
use crate::snapshot::RuntimeSnapshot;
use crate::transaction::Transaction;
use conduit_metrics::MetricsHub;
use std::sync::Arc;

/// Runs when Lookup (or duration abort) produced no response wire.
#[derive(Default)]
pub struct NoAnswerStage {
    pub metrics: Option<Arc<MetricsHub>>,
}

impl NoAnswerStage {
    pub fn new(metrics: Option<Arc<MetricsHub>>) -> Self {
        Self { metrics }
    }
}

impl PipelineStage for NoAnswerStage {
    fn name(&self) -> &'static str {
        "no_answer"
    }

    fn handle(&self, txn: &mut Transaction, snapshot: &Arc<RuntimeSnapshot>) -> StageOutcome {
        if let Some(hub) = self.metrics.as_ref() {
            if hub.metrics_enabled() {
                let profile = txn.lookup_profile_name();
                let reason = txn
                    .convergence_reason
                    .map(|r| r.as_str())
                    .unwrap_or("unknown");
                let pool = txn.selected_pool.as_deref().unwrap_or("");
                txn.builtin_registry(hub)
                    .record_lookup_no_answer(profile, reason, pool);
            }
        }

        let result = snapshot.rules.eval(
            RuleHook::NoAnswer,
            txn,
            &snapshot.scripting,
            self.metrics.as_deref(),
            None,
        );

        match result.outcome {
            RuleOutcome::Drop => StageOutcome::Drop,
            RuleOutcome::Retry => {
                debug_assert!(
                    false,
                    "no_answer rules must not produce retry at compile or runtime"
                );
                StageOutcome::Continue(Phase::Send)
            }
            RuleOutcome::Continue => StageOutcome::Continue(Phase::Send),
        }
    }
}
