//! WaitResponse checkpoint after upstream I/O completes.

use conduit_core::phase::Phase;
use conduit_core::pipeline::{PipelineStage, StageOutcome};
use conduit_core::record_upstream_response;
use conduit_core::snapshot::RuntimeSnapshot;
use conduit_core::Transaction;
use std::sync::Arc;

/// Resume checkpoint: upstream reply or timeout is already on the transaction.
pub struct WaitResponseStage {
    parse_wire_meta: bool,
}

impl WaitResponseStage {
    pub fn new(parse_wire_meta: bool) -> Self {
        Self { parse_wire_meta }
    }
}

impl PipelineStage for WaitResponseStage {
    fn name(&self) -> &'static str {
        "wait_response"
    }

    fn handle(&self, txn: &mut Transaction, _snapshot: &Arc<RuntimeSnapshot>) -> StageOutcome {
        if let Some(wire) = txn.response_wire.clone() {
            record_upstream_response(txn, &wire, self.parse_wire_meta);
        }
        StageOutcome::Continue(Phase::ResponseRules)
    }
}
