//! Graceful drain of in-flight transaction slots.
//!
//! Drain waits until all matching non-`Free` slots (including parked `IoWait`
//! legs) return to the free list, or a timeout elapses. This is the slot-side
//! preparation for the future `zero-downtime-upgrade` handoff; no process
//! handoff is implemented here.
//!
//! ## `reuse_port` interaction
//!
//! When ingress listeners are bound with `reuse_port`, a replacement process
//! can bind the same address while this process drains: the kernel steers new
//! traffic to the new socket(s) and in-flight slots here finish undisturbed.
//! Draining the slot pool is therefore independent of unbinding listeners.
//!
//! ## Filter scope
//!
//! [`DrainFilter`] currently selects by [`ClientProtocol`] (UDP vs TCP). The
//! default (no filter) drains all slots. Listener-scoped draining
//! (by listener name/label) is deferred until slots carry a listener identity;
//! it is tracked as a future feature for `zero-downtime-upgrade`.

use conduit_core::txn_store::{SharedTxnStore, TxnSlot};
use conduit_core::ClientProtocol;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

/// Default drain wait when no explicit timeout is given. Mirrors
/// `conduit_config::DEFAULT_DRAIN_TIMEOUT_MS` (`shutdown.drain_timeout_ms`).
pub const DEFAULT_DRAIN_TIMEOUT: Duration = Duration::from_secs(5);

/// Poll interval while waiting for slots to reach a terminal/free state.
const DRAIN_POLL_INTERVAL: Duration = Duration::from_millis(10);

/// Selects which in-flight slots a drain waits on. Default = all slots.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DrainFilter {
    /// Restrict to a single client protocol; `None` matches every protocol.
    pub protocol: Option<ClientProtocol>,
}

impl DrainFilter {
    /// Drain only slots for the given client protocol.
    pub fn protocol(protocol: ClientProtocol) -> Self {
        Self {
            protocol: Some(protocol),
        }
    }

    fn matches(&self, slot: &TxnSlot) -> bool {
        match self.protocol {
            Some(p) => slot.txn.protocol == p,
            None => true,
        }
    }
}

/// Result of a drain attempt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DrainOutcome {
    /// All matching slots reached a terminal/free state within the timeout.
    Drained,
    /// Timeout elapsed with `remaining` matching slots still in flight.
    TimedOut { remaining: u32 },
    /// The caller cancelled the wait (e.g. a second shutdown signal) with
    /// `remaining` matching slots still in flight.
    Aborted { remaining: u32 },
}

impl DrainOutcome {
    pub fn is_drained(self) -> bool {
        matches!(self, DrainOutcome::Drained)
    }
}

/// Wait until matching non-`Free` slots drain, or `timeout` elapses, or the
/// caller-supplied `cancel` flag is set.
///
/// A `None` filter drains all slots. A zero timeout performs a single check.
/// When `cancel` is provided and becomes `true` while slots are still in
/// flight, the wait returns [`DrainOutcome::Aborted`] (used to let a second
/// `SIGINT`/`SIGTERM` skip the remaining drain wait and exit promptly).
pub fn drain_slots(
    txn_store: &SharedTxnStore,
    timeout: Duration,
    filter: Option<DrainFilter>,
    cancel: Option<&AtomicBool>,
) -> DrainOutcome {
    let filter = filter.unwrap_or_default();
    let deadline = Instant::now() + timeout;
    loop {
        let remaining = txn_store.active_slots_matching(|slot| filter.matches(slot));
        if remaining == 0 {
            return DrainOutcome::Drained;
        }
        if cancel.is_some_and(|c| c.load(Ordering::Relaxed)) {
            return DrainOutcome::Aborted { remaining };
        }
        let now = Instant::now();
        if now >= deadline {
            return DrainOutcome::TimedOut { remaining };
        }
        let sleep = DRAIN_POLL_INTERVAL.min(deadline.saturating_duration_since(now));
        std::thread::sleep(sleep);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use conduit_core::txn_store::{SharedTxnStore, SlotId, SlotState};

    /// Acquire a slot and tag it with a client protocol, leaving it active.
    fn acquire_active(store: &SharedTxnStore, protocol: ClientProtocol) -> SlotId {
        let id = store.acquire().expect("slot available");
        store
            .with_slot(id, SlotState::Ingress, |slot| {
                slot.txn.protocol = protocol;
                Ok(())
            })
            .expect("tag protocol");
        id
    }

    fn release(store: &SharedTxnStore, id: SlotId) {
        store.release_active(id).expect("release");
    }

    #[test]
    fn drain_returns_immediately_when_no_active_slots() {
        let store = SharedTxnStore::new(8, 4);
        assert_eq!(
            drain_slots(&store, Duration::from_millis(0), None, None),
            DrainOutcome::Drained
        );
    }

    #[test]
    fn udp_only_drain_ignores_tcp_slots() {
        let store = SharedTxnStore::new(8, 4);
        let _tcp = acquire_active(&store, ClientProtocol::Tcp);
        let udp = acquire_active(&store, ClientProtocol::Udp);

        // UDP filter still sees the in-flight UDP slot.
        assert_eq!(
            drain_slots(
                &store,
                Duration::from_millis(0),
                Some(DrainFilter::protocol(ClientProtocol::Udp)),
                None,
            ),
            DrainOutcome::TimedOut { remaining: 1 }
        );

        // Once the UDP slot finishes, a UDP-only drain completes even though the
        // TCP slot is still active.
        release(&store, udp);
        assert_eq!(
            drain_slots(
                &store,
                Duration::from_millis(50),
                Some(DrainFilter::protocol(ClientProtocol::Udp)),
                None,
            ),
            DrainOutcome::Drained
        );

        // A full (default) drain still reports the outstanding TCP slot.
        assert_eq!(
            drain_slots(&store, Duration::from_millis(0), None, None),
            DrainOutcome::TimedOut { remaining: 1 }
        );
    }

    #[test]
    fn drain_completes_when_slot_released_concurrently() {
        let store = SharedTxnStore::new(8, 4);
        let id = acquire_active(&store, ClientProtocol::Udp);

        let releaser = {
            let store = store.clone();
            std::thread::spawn(move || {
                std::thread::sleep(Duration::from_millis(20));
                release(&store, id);
            })
        };

        assert_eq!(
            drain_slots(&store, Duration::from_secs(2), None, None),
            DrainOutcome::Drained
        );
        releaser.join().unwrap();
    }

    #[test]
    fn drain_aborts_when_cancel_flag_set() {
        let store = SharedTxnStore::new(8, 4);
        let _active = acquire_active(&store, ClientProtocol::Udp);
        let cancel = AtomicBool::new(true);
        // Long timeout, but the pre-set cancel flag forces an immediate abort.
        assert_eq!(
            drain_slots(&store, Duration::from_secs(60), None, Some(&cancel)),
            DrainOutcome::Aborted { remaining: 1 }
        );
    }

    #[test]
    fn drain_ignores_cancel_when_already_drained() {
        let store = SharedTxnStore::new(8, 4);
        let cancel = AtomicBool::new(true);
        // No active slots: success takes precedence over the cancel flag.
        assert_eq!(
            drain_slots(&store, Duration::from_millis(0), None, Some(&cancel)),
            DrainOutcome::Drained
        );
    }
}
