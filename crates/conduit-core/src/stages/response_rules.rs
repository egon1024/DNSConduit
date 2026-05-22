//! ResponseRules hook — built-in rule evaluation and retry intent.

use crate::phase::Phase;
use crate::pipeline::{PipelineStage, StageOutcome};
use crate::rules::{RuleHook, RuleOutcome};
use crate::snapshot::RuntimeSnapshot;
use crate::transaction::Transaction;
use std::sync::Arc;

pub struct ResponseRulesStage;

impl PipelineStage for ResponseRulesStage {
    fn name(&self) -> &'static str {
        "response_rules"
    }

    fn handle(&self, txn: &mut Transaction, snapshot: &Arc<RuntimeSnapshot>) -> StageOutcome {
        match snapshot.rules.eval(RuleHook::Response, txn) {
            RuleOutcome::Drop => StageOutcome::Drop,
            RuleOutcome::Retry => {
                if txn.retry_pool.is_some() {
                    StageOutcome::Continue(Phase::Route)
                } else {
                    StageOutcome::Continue(Phase::Send)
                }
            }
            RuleOutcome::Continue => StageOutcome::Continue(Phase::Send),
        }
    }
}
