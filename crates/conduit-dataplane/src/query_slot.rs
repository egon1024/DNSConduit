//! Run orchestrator work inside a transaction slot (sync runtime).

use conduit_core::orchestrator::{Orchestrator, RunOutcome};
use conduit_core::snapshot::RuntimeSnapshot;
use conduit_core::txn_store::{AcquireError, SharedTxnStore, SlotState, TxnSlot};
use conduit_core::SystemClock;
use conduit_events::EventHub;
use std::sync::Arc;

/// Acquire a slot, run the orchestrator, and release the slot.
///
/// Returns `Ok(None)` when the query is dropped or the slot pool is exhausted.
pub fn run_in_slot(
    txn_store: &SharedTxnStore,
    orchestrator: &Orchestrator,
    snap: &Arc<RuntimeSnapshot>,
    observation: &EventHub,
    setup: impl FnOnce(&mut TxnSlot),
) -> Result<Option<Vec<u8>>, AcquireError> {
    let mut store = txn_store.lock();
    let slot_id = store.acquire()?;
    if store
        .transition(slot_id, SlotState::Ingress, SlotState::Policy)
        .is_err()
    {
        let _ = store.release_active(slot_id);
        return Ok(None);
    }

    let outcome = store.with_slot(slot_id, SlotState::Policy, |slot| {
        setup(slot);
        Ok(orchestrator.run(&mut slot.txn, snap, &SystemClock, Some(observation)))
    });

    let wire = match outcome {
        Ok(RunOutcome::Response(w)) => Some(w),
        Ok(RunOutcome::Dropped) | Err(_) => None,
    };
    let _ = store.release_active(slot_id);
    Ok(wire)
}
