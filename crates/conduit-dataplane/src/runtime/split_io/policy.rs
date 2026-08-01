//! Policy worker pool for split_io.

use super::queue::{deliver_reply, PolicyQueue, PolicyWork, ReplyRoutes};
use crate::forward::{apply_wait_completion, PoolInflight};
use crate::listener::DataplaneShutdown;
use conduit_core::orchestrator::{Orchestrator, OrchestratorRun, RunOutcome};
use conduit_core::phase::Phase;
use conduit_core::snapshot::SnapshotStore;
use conduit_core::txn_store::{SharedTxnStore, SlotId, SlotState};
use conduit_core::SystemClock;
use conduit_events::EventHub;
use std::sync::Arc;

#[allow(clippy::too_many_arguments)]
pub fn run_policy_worker(
    queue: Arc<PolicyQueue>,
    home_shard: usize,
    txn_store: SharedTxnStore,
    orchestrator: Arc<Orchestrator>,
    store: Arc<SnapshotStore>,
    events: Arc<EventHub>,
    reply_routes: Arc<ReplyRoutes>,
    inflight: Arc<PoolInflight>,
    shutdown: DataplaneShutdown,
) {
    while let Some(work) = queue.pop(home_shard, &shutdown) {
        match work {
            PolicyWork::New(slot_id) => process_new(
                slot_id,
                &txn_store,
                &orchestrator,
                &store,
                &events,
                &reply_routes,
                &inflight,
            ),
            PolicyWork::Resume(resume) => process_resume(
                resume,
                &txn_store,
                &orchestrator,
                &store,
                &events,
                &reply_routes,
                &inflight,
            ),
            PolicyWork::LookupResume(slot_id) => process_lookup_resume(
                slot_id,
                &txn_store,
                &orchestrator,
                &store,
                &events,
                &reply_routes,
                &inflight,
            ),
        }
    }
}

fn process_new(
    slot_id: SlotId,
    txn_store: &SharedTxnStore,
    orchestrator: &Orchestrator,
    store: &Arc<SnapshotStore>,
    events: &Arc<EventHub>,
    reply_routes: &ReplyRoutes,
    inflight: &PoolInflight,
) {
    if txn_store
        .transition(slot_id, SlotState::Ingress, SlotState::Policy)
        .is_err()
    {
        let _ = txn_store.release_active(slot_id);
        return;
    }
    run_policy_loop(
        slot_id,
        txn_store,
        orchestrator,
        store,
        events,
        reply_routes,
        inflight,
        None,
    );
}

fn process_resume(
    resume: crate::forward::IoResume,
    txn_store: &SharedTxnStore,
    orchestrator: &Orchestrator,
    store: &Arc<SnapshotStore>,
    events: &Arc<EventHub>,
    reply_routes: &ReplyRoutes,
    inflight: &PoolInflight,
) {
    let slot_id = resume.slot_id;
    // Slot-scoped exclusion: Resume IoWait→Policy + apply completion under the
    // same per-slot lock so it cannot interleave with Policy→IoWait publish on
    // this slot (fast-upstream race). Other slots remain free to progress.
    let ok = txn_store.with_slot_exclusive(slot_id, |slot| {
        if slot.state != SlotState::IoWait {
            return Ok(false);
        }
        slot.state = SlotState::Policy;
        apply_wait_completion(&mut slot.txn, &resume.completion);
        Ok(true)
    });
    match ok {
        Ok(true) => {}
        Ok(false) => {
            tracing::warn!(
                slot = slot_id.index(),
                "split_io: dropped I/O resume; slot was not in IoWait (possible lost client reply)"
            );
            return;
        }
        Err(_) => return,
    }
    run_policy_loop(
        slot_id,
        txn_store,
        orchestrator,
        store,
        events,
        reply_routes,
        inflight,
        Some(Phase::Lookup),
    );
}

fn process_lookup_resume(
    slot_id: SlotId,
    txn_store: &SharedTxnStore,
    orchestrator: &Orchestrator,
    store: &Arc<SnapshotStore>,
    events: &Arc<EventHub>,
    reply_routes: &ReplyRoutes,
    inflight: &PoolInflight,
) {
    if txn_store
        .transition(slot_id, SlotState::IoWait, SlotState::Policy)
        .is_err()
    {
        return;
    }
    run_policy_loop(
        slot_id,
        txn_store,
        orchestrator,
        store,
        events,
        reply_routes,
        inflight,
        Some(Phase::Lookup),
    );
}

#[allow(clippy::too_many_arguments)]
fn run_policy_loop(
    slot_id: SlotId,
    txn_store: &SharedTxnStore,
    orchestrator: &Orchestrator,
    store: &Arc<SnapshotStore>,
    events: &Arc<EventHub>,
    reply_routes: &ReplyRoutes,
    inflight: &PoolInflight,
    mut resume_phase: Option<Phase>,
) {
    let snap = store.load();
    // Hold this *slot's* mutex across run_until_suspend/resume_after_suspend *and*
    // the Policy→IoWait transition. With a fast upstream, the I/O poller can
    // enqueue a Resume before this worker would otherwise re-acquire the slot
    // lock; another policy worker would then fail IoWait→Policy and drop the
    // resume, leaving the slot parked forever. Distinct slots use distinct
    // mutexes, so policy_workers > 1 can still run orchestrator work in parallel.
    let step = match txn_store.with_slot_exclusive(slot_id, |slot| {
        if slot.state != SlotState::Policy {
            return Err(conduit_core::txn_store::SlotError::StateMismatch {
                expected: SlotState::Policy,
                actual: slot.state,
            });
        }
        let step = if let Some(phase) = resume_phase.take() {
            orchestrator.resume_after_suspend(
                &mut slot.txn,
                &snap,
                &SystemClock,
                Some(events.as_ref()),
                phase,
            )
        } else {
            orchestrator.run_until_suspend(
                &mut slot.txn,
                &snap,
                &SystemClock,
                Some(events.as_ref()),
            )
        };
        if matches!(&step, OrchestratorRun::Suspended { .. }) {
            slot.state = SlotState::IoWait;
        }
        Ok(step)
    }) {
        Ok(s) => s,
        Err(_) => {
            release_slot(txn_store, slot_id, inflight);
            return;
        }
    };

    match step {
        OrchestratorRun::Finished(RunOutcome::Dropped) => {
            release_slot(txn_store, slot_id, inflight);
        }
        OrchestratorRun::Finished(RunOutcome::Response(wire)) => {
            deliver_reply(reply_routes, slot_id, wire);
            release_slot(txn_store, slot_id, inflight);
        }
        OrchestratorRun::Suspended { .. } => {
            // Policy→IoWait already applied under the same per-slot lock as suspend.
        }
    }
}

fn release_slot(txn_store: &SharedTxnStore, slot_id: SlotId, inflight: &PoolInflight) {
    let pool = txn_store
        .with_slot_exclusive(slot_id, |slot| {
            let pool = if matches!(slot.state, SlotState::Policy | SlotState::IoWait) {
                slot.txn.selected_pool.clone()
            } else {
                None
            };
            Ok(pool)
        })
        .ok()
        .flatten();
    let _ = txn_store.release_active(slot_id);
    if let Some(pool) = pool {
        inflight.release(&pool);
    }
}
