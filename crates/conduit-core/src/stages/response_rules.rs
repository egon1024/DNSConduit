//! ResponseRules hook — built-in rule evaluation, Rhai scripts, and retry intent.

use crate::phase::Phase;
use crate::pipeline::{PipelineStage, StageOutcome};
use crate::rules::{RuleEvalResult, RuleHook, RuleOutcome};
use crate::snapshot::RuntimeSnapshot;
use crate::transaction::Transaction;
use conduit_metrics::MetricsHub;
use conduit_script::{run_scripts, ScriptPhase, ScriptRunOutcome};
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
        let RuleEvalResult {
            outcome,
            matched_rule_id,
        } = snapshot.rules.eval(RuleHook::Response, txn);

        let mut script_retry = false;
        if let Some(rule_id) = matched_rule_id {
            let script_ids = snapshot
                .scripting
                .script_ids_for_rule(&rule_id, ScriptPhase::Response);
            if !script_ids.is_empty() {
                let user_export = self.metrics.as_ref().map(|m| m.user.as_ref());
                let (script_outcome, _) = run_scripts(
                    &snapshot.scripting,
                    &script_ids,
                    txn,
                    ScriptPhase::Response,
                    user_export,
                );
                match script_outcome {
                    ScriptRunOutcome::Drop => return StageOutcome::Drop,
                    ScriptRunOutcome::Retry => script_retry = true,
                    ScriptRunOutcome::Ok | ScriptRunOutcome::Error => {}
                }
            }
        }

        if script_retry || outcome == RuleOutcome::Retry {
            if txn.retry_pool.is_some() {
                return StageOutcome::Continue(Phase::Route);
            }
            return StageOutcome::Continue(Phase::Send);
        }

        match outcome {
            RuleOutcome::Drop => StageOutcome::Drop,
            RuleOutcome::Continue => StageOutcome::Continue(Phase::Send),
            RuleOutcome::Retry => StageOutcome::Continue(Phase::Send),
        }
    }
}
