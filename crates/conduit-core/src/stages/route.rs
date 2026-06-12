//! Route phase — select pool and weighted backend.

use crate::phase::Phase;
use crate::pipeline::{PipelineStage, StageOutcome};
use crate::routing::{default_pool_name, select_backend, tried_backends_in_pool};
use crate::snapshot::RuntimeSnapshot;
use crate::transaction::Transaction;
use std::sync::Arc;

pub struct RouteStage;

impl PipelineStage for RouteStage {
    fn name(&self) -> &'static str {
        "route"
    }

    fn handle(&self, txn: &mut Transaction, snapshot: &Arc<RuntimeSnapshot>) -> StageOutcome {
        let pool_name = txn
            .take_retry_pool()
            .or_else(|| txn.selected_pool.clone())
            .or_else(|| default_pool_name(&snapshot.config));

        let Some(pool_name) = pool_name else {
            txn.set_rcode_name("SERVFAIL");
            return StageOutcome::Continue(Phase::Send);
        };

        let tried = tried_backends_in_pool(&txn.attempts, &pool_name);
        let Some((pool, backend)) = select_backend(
            &snapshot.config.pools,
            &pool_name,
            txn.id,
            snapshot.generation,
            txn.attempt_count,
            &tried,
        ) else {
            txn.set_rcode_name("SERVFAIL");
            return StageOutcome::Continue(Phase::Send);
        };

        txn.record_attempt(pool.clone(), backend);
        tracing::debug!(
            txn_id = txn.id,
            dns_id = txn.dns_id,
            pool = %pool,
            %backend,
            attempt = txn.attempt_count,
            "route selected backend"
        );
        StageOutcome::Continue(Phase::Forward)
    }
}
