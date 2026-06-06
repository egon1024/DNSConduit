//! RequestRules hook — built-in rule evaluation, then Rhai scripts.

use crate::phase::Phase;
use crate::pipeline::{PipelineStage, StageOutcome};
use crate::rules::{RuleEvalResult, RuleHook, RuleOutcome};
use crate::snapshot::RuntimeSnapshot;
use crate::transaction::Transaction;
use conduit_metrics::MetricsHub;
use conduit_script::{run_scripts, ScriptPhase, ScriptRunOutcome};
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
        let RuleEvalResult {
            outcome,
            matched_rule_name,
        } = snapshot.rules.eval(RuleHook::Request, txn);

        if let Some(rule_name) = matched_rule_name {
            let script_ids = snapshot
                .scripting
                .script_ids_for_rule(&rule_name, ScriptPhase::Request);
            if !script_ids.is_empty() {
                let user_export = self.metrics.as_ref().map(|m| m.user.as_ref());
                let (script_outcome, _) = run_scripts(
                    &snapshot.scripting,
                    &script_ids,
                    txn,
                    ScriptPhase::Request,
                    user_export,
                );
                match script_outcome {
                    ScriptRunOutcome::Drop => return StageOutcome::Drop,
                    ScriptRunOutcome::Retry => {}
                    ScriptRunOutcome::Ok | ScriptRunOutcome::Error => {}
                }
            }
        }

        match outcome {
            RuleOutcome::Drop => StageOutcome::Drop,
            RuleOutcome::Continue | RuleOutcome::Retry => StageOutcome::Continue(Phase::Route),
        }
    }
}
