//! RequestRules hook — built-in rule evaluation.

use crate::phase::Phase;
use crate::pipeline::{PipelineStage, StageOutcome};
use crate::rules::{RuleHook, RuleOutcome};
use crate::snapshot::RuntimeSnapshot;
use crate::transaction::Transaction;
use std::sync::Arc;

pub struct RequestRulesStage;

impl PipelineStage for RequestRulesStage {
    fn name(&self) -> &'static str {
        "request_rules"
    }

    fn handle(&self, txn: &mut Transaction, snapshot: &Arc<RuntimeSnapshot>) -> StageOutcome {
        match snapshot.rules.eval(RuleHook::Request, txn) {
            RuleOutcome::Drop => StageOutcome::Drop,
            RuleOutcome::Continue | RuleOutcome::Retry => StageOutcome::Continue(Phase::Route),
        }
    }
}
