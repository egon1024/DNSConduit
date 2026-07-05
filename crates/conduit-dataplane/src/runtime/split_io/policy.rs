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
    txn_store: SharedTxnStore,
    orchestrator: Arc<Orchestrator>,
    store: Arc<SnapshotStore>,
    events: Arc<EventHub>,
    reply_routes: Arc<ReplyRoutes>,
    inflight: Arc<PoolInflight>,
    shutdown: DataplaneShutdown,
) {
    while let Some(work) = queue.pop(&shutdown) {
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
    {
        let mut guard = txn_store.lock();
        if guard
            .transition(slot_id, SlotState::Ingress, SlotState::Policy)
            .is_err()
        {
            let _ = guard.release_active(slot_id);
            return;
        }
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
    {
        let mut guard = txn_store.lock();
        if guard
            .transition(slot_id, SlotState::IoWait, SlotState::Policy)
            .is_err()
        {
            return;
        }
        let _ = guard.with_slot(slot_id, SlotState::Policy, |slot| {
            apply_wait_completion(&mut slot.txn, &resume.completion);
            Ok(())
        });
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
    let step = {
        let mut guard = txn_store.lock();
        guard.with_slot(slot_id, SlotState::Policy, |slot| {
            Ok(if let Some(phase) = resume_phase.take() {
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
            })
        })
    };

    let step = match step {
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
            let mut guard = txn_store.lock();
            let _ = guard.transition(slot_id, SlotState::Policy, SlotState::IoWait);
        }
    }
}

fn release_slot(txn_store: &SharedTxnStore, slot_id: SlotId, inflight: &PoolInflight) {
    let pool = {
        let mut guard = txn_store.lock();
        let pool = guard
            .with_slot(slot_id, SlotState::Policy, |slot| {
                Ok(slot.txn.selected_pool.clone())
            })
            .or_else(|_| {
                guard.with_slot(slot_id, SlotState::IoWait, |slot| {
                    Ok(slot.txn.selected_pool.clone())
                })
            })
            .ok()
            .flatten();
        let _ = guard.release_active(slot_id);
        pool
    };
    if let Some(pool) = pool {
        inflight.release(&pool);
    }
}
