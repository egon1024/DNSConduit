//! Lookup orchestrator stage — profile-ordered provider chain.

use crate::lookup::{AnswerSource, LookupOutcome};
use crate::phase::Phase;
use crate::pipeline::{PipelineStage, StageOutcome};
use crate::snapshot::RuntimeSnapshot;
use crate::transaction::Transaction;
use conduit_config::lookup::{CompiledLookupProvider, DEFAULT_LOOKUP_PROFILE};
use conduit_metrics::MetricsHub;
use std::sync::Arc;

/// Resume point inside the forward provider after async upstream I/O.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LookupForwardStep {
    Wait,
}

/// Runs lookup providers for the active profile. The forward provider internalizes
/// Route → Forward → WaitResponse without exposing those as top-level graph phases.
pub struct LookupStage {
    route: Arc<dyn PipelineStage>,
    forward: Arc<dyn PipelineStage>,
    wait: Arc<dyn PipelineStage>,
    metrics: Option<Arc<MetricsHub>>,
}

impl LookupStage {
    pub fn new(
        route: Arc<dyn PipelineStage>,
        forward: Arc<dyn PipelineStage>,
        wait: Arc<dyn PipelineStage>,
        metrics: Option<Arc<MetricsHub>>,
    ) -> Self {
        Self {
            route,
            forward,
            wait,
            metrics,
        }
    }

    fn run_forward_chain(
        &self,
        txn: &mut Transaction,
        snapshot: &Arc<RuntimeSnapshot>,
    ) -> StageOutcome {
        txn.cache_lookup_eligible = false;

        let max_attempts = snapshot
            .config
            .orchestrator
            .as_ref()
            .map(|o| o.max_attempts)
            .unwrap_or(3);
        if txn.attempt_count >= max_attempts {
            txn.set_rcode_name("SERVFAIL");
            return StageOutcome::Continue(Phase::Send);
        }

        let route_out = self.route.handle(txn, snapshot);
        match route_out {
            StageOutcome::Continue(Phase::Forward) => {}
            StageOutcome::Continue(Phase::Send) => return StageOutcome::Continue(Phase::Send),
            StageOutcome::Drop => return StageOutcome::Drop,
            other => {
                tracing::warn!(?other, "unexpected route outcome inside lookup");
                return StageOutcome::Continue(Phase::Send);
            }
        }

        if let Some(hub) = self.metrics.as_ref() {
            if hub.metrics_enabled() {
                if let Some(ref pool) = txn.selected_pool {
                    hub.builtin.record_query_by_pool(pool);
                }
            }
        }

        let forward_out = self.forward.handle(txn, snapshot);
        match forward_out {
            StageOutcome::Suspend(Phase::WaitResponse) => {
                txn.lookup_forward_step = Some(LookupForwardStep::Wait);
                StageOutcome::Suspend(Phase::Lookup)
            }
            StageOutcome::Continue(Phase::WaitResponse) => {
                self.finish_forward_answer(txn, snapshot)
            }
            StageOutcome::Continue(Phase::ResponseRules) => {
                txn.lookup_outcome = Some(LookupOutcome::Answered);
                txn.answer_source = Some(AnswerSource::Forward);
                StageOutcome::Continue(Phase::ResponseRules)
            }
            StageOutcome::Continue(Phase::Send) => StageOutcome::Continue(Phase::Send),
            StageOutcome::Drop => StageOutcome::Drop,
            other => {
                tracing::warn!(?other, "unexpected forward outcome inside lookup");
                StageOutcome::Continue(Phase::Send)
            }
        }
    }

    fn finish_forward_answer(
        &self,
        txn: &mut Transaction,
        snapshot: &Arc<RuntimeSnapshot>,
    ) -> StageOutcome {
        let wait_out = self.wait.handle(txn, snapshot);
        match wait_out {
            StageOutcome::Continue(Phase::ResponseRules) => {
                txn.lookup_outcome = Some(LookupOutcome::Answered);
                txn.answer_source = Some(AnswerSource::Forward);
                StageOutcome::Continue(Phase::ResponseRules)
            }
            StageOutcome::Continue(Phase::Send) => StageOutcome::Continue(Phase::Send),
            StageOutcome::Drop => StageOutcome::Drop,
            other => {
                tracing::warn!(?other, "unexpected wait outcome inside lookup");
                StageOutcome::Continue(Phase::Send)
            }
        }
    }
}

impl PipelineStage for LookupStage {
    fn name(&self) -> &'static str {
        "lookup"
    }

    fn handle(&self, txn: &mut Transaction, snapshot: &Arc<RuntimeSnapshot>) -> StageOutcome {
        if txn.lookup_forward_step == Some(LookupForwardStep::Wait) {
            txn.lookup_forward_step = None;
            return self.finish_forward_answer(txn, snapshot);
        }

        let profile_name = txn
            .lookup_profile
            .as_deref()
            .unwrap_or(DEFAULT_LOOKUP_PROFILE);
        let Some(profile) = snapshot.lookup.profiles.get(profile_name) else {
            tracing::error!(profile = profile_name, "unknown lookup profile");
            txn.set_rcode_name("SERVFAIL");
            return StageOutcome::Continue(Phase::Send);
        };
        txn.lookup_profile = Some(profile.name.clone());

        for provider in &profile.providers {
            match provider {
                CompiledLookupProvider::Cache { .. } => {
                    if !txn.cache_lookup_eligible {
                        continue;
                    }
                    // Cache provider ships in Phase C; treat as miss for forward-only parity.
                    txn.lookup_outcome = Some(LookupOutcome::Miss);
                }
                CompiledLookupProvider::Forward => {
                    return self.run_forward_chain(txn, snapshot);
                }
            }
        }

        txn.lookup_outcome = Some(LookupOutcome::Miss);
        StageOutcome::Continue(Phase::ResponseRules)
    }
}
