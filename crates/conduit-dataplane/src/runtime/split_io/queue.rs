//! Work queues and reply routing for split_io.
//!
//! Both structures are sharded by sticky `slot_id` so concurrent ingress
//! producers and policy consumers only contend on the shard that owns a given
//! slot — not on one process-wide mutex.

use crate::forward::IoResume;
use conduit_core::txn_store::SlotId;
use std::collections::{HashMap, VecDeque};
use std::net::{SocketAddr, UdpSocket};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

use crate::listener::DataplaneShutdown;

/// Upper bound on queue / reply-route shard count (startup derivation).
pub const MAX_QUEUE_SHARDS: usize = 64;

/// Derive shard count at split_io startup: power-of-two ≥ max(4, policy_workers),
/// capped at [`MAX_QUEUE_SHARDS`]. Always at least 1.
///
/// No operator knob for MVP — scales with the policy consumer pool.
pub fn derive_shard_count(policy_workers: u32) -> usize {
    let workers = (policy_workers as usize).max(1);
    let n = workers.max(4);
    n.next_power_of_two().clamp(1, MAX_QUEUE_SHARDS)
}

/// Policy pool work item.
#[derive(Debug, Clone)]
pub enum PolicyWork {
    New(SlotId),
    Resume(IoResume),
    /// Cache single-flight coalesce completed; resume Lookup for a parked slot.
    LookupResume(SlotId),
}

impl PolicyWork {
    pub fn slot_id(&self) -> SlotId {
        match self {
            PolicyWork::New(id) | PolicyWork::LookupResume(id) => *id,
            PolicyWork::Resume(resume) => resume.slot_id,
        }
    }
}

struct QueueShard {
    inner: Mutex<VecDeque<PolicyWork>>,
    cv: Condvar,
}

impl QueueShard {
    fn new() -> Self {
        Self {
            inner: Mutex::new(VecDeque::new()),
            cv: Condvar::new(),
        }
    }
}

pub struct PolicyQueue {
    shards: Box<[QueueShard]>,
}

impl PolicyQueue {
    /// Build a queue with the given shard count (must be ≥ 1).
    pub fn with_shards(shard_count: usize) -> Self {
        let n = shard_count.max(1);
        let shards = (0..n).map(|_| QueueShard::new()).collect::<Vec<_>>();
        Self {
            shards: shards.into_boxed_slice(),
        }
    }

    /// Convenience for tests / single-shard callers.
    pub fn new() -> Self {
        Self::with_shards(1)
    }

    pub fn shard_count(&self) -> usize {
        self.shards.len()
    }

    /// Sticky shard index for a slot (same rule as [`ReplyRoutes`]).
    pub fn shard_for_slot(&self, slot_id: SlotId) -> usize {
        (slot_id.index() as usize) % self.shards.len()
    }

    pub fn push(&self, work: PolicyWork) {
        let idx = self.shard_for_slot(work.slot_id());
        let shard = &self.shards[idx];
        shard.inner.lock().unwrap().push_back(work);
        shard.cv.notify_one();
    }

    fn try_pop_shard(&self, idx: usize) -> Option<PolicyWork> {
        let mut guard = self.shards[idx].inner.lock().unwrap();
        guard.pop_front()
    }

    /// Pop preferring `home_shard`, then steal from other shards. Waiters use a
    /// short timeout so they can steal work pushed to non-home shards.
    pub fn pop(&self, home_shard: usize, shutdown: &DataplaneShutdown) -> Option<PolicyWork> {
        let n = self.shards.len();
        let home = home_shard % n;
        loop {
            if shutdown.is_shutdown() {
                return None;
            }
            if let Some(work) = self.try_pop_shard(home) {
                return Some(work);
            }
            for offset in 1..n {
                let idx = (home + offset) % n;
                if let Some(work) = self.try_pop_shard(idx) {
                    return Some(work);
                }
            }
            let shard = &self.shards[home];
            let guard = shard.inner.lock().unwrap();
            if shutdown.is_shutdown() {
                return None;
            }
            if !guard.is_empty() {
                // Work arrived on home while we scanned; retry without waiting.
                drop(guard);
                continue;
            }
            let _ = shard
                .cv
                .wait_timeout(guard, Duration::from_millis(100))
                .unwrap();
        }
    }

    /// Test helper: hold one shard's mutex while `f` runs so concurrent pushes
    /// to other shards can prove they do not need this lock.
    #[cfg(test)]
    pub(crate) fn with_shard_locked<R>(&self, shard: usize, f: impl FnOnce() -> R) -> R {
        let _guard = self.shards[shard % self.shards.len()].inner.lock().unwrap();
        f()
    }
}

impl Default for PolicyQueue {
    fn default() -> Self {
        Self::new()
    }
}

/// Where to deliver the client response for a slot.
#[derive(Debug)]
pub enum ReplyTarget {
    Udp {
        socket: Arc<UdpSocket>,
        peer: SocketAddr,
    },
    Tcp {
        tx: crossbeam_channel::Sender<Vec<u8>>,
    },
}

struct RouteShard {
    inner: Mutex<HashMap<SlotId, ReplyTarget>>,
}

impl RouteShard {
    fn new() -> Self {
        Self {
            inner: Mutex::new(HashMap::new()),
        }
    }
}

pub struct ReplyRoutes {
    shards: Box<[RouteShard]>,
}

impl ReplyRoutes {
    pub fn with_shards(shard_count: usize) -> Self {
        let n = shard_count.max(1);
        let shards = (0..n).map(|_| RouteShard::new()).collect::<Vec<_>>();
        Self {
            shards: shards.into_boxed_slice(),
        }
    }

    pub fn new() -> Self {
        Self::with_shards(1)
    }

    pub fn shard_count(&self) -> usize {
        self.shards.len()
    }

    pub fn shard_for_slot(&self, slot_id: SlotId) -> usize {
        (slot_id.index() as usize) % self.shards.len()
    }

    pub fn insert(&self, slot_id: SlotId, target: ReplyTarget) {
        let idx = self.shard_for_slot(slot_id);
        self.shards[idx]
            .inner
            .lock()
            .unwrap()
            .insert(slot_id, target);
    }

    pub fn take(&self, slot_id: SlotId) -> Option<ReplyTarget> {
        let idx = self.shard_for_slot(slot_id);
        self.shards[idx].inner.lock().unwrap().remove(&slot_id)
    }

    #[cfg(test)]
    pub(crate) fn with_shard_locked<R>(&self, shard: usize, f: impl FnOnce() -> R) -> R {
        let _guard = self.shards[shard % self.shards.len()].inner.lock().unwrap();
        f()
    }
}

impl Default for ReplyRoutes {
    fn default() -> Self {
        Self::new()
    }
}

pub fn deliver_reply(routes: &ReplyRoutes, slot_id: SlotId, wire: Vec<u8>) {
    let Some(target) = routes.take(slot_id) else {
        return;
    };
    match target {
        ReplyTarget::Udp { socket, peer } => {
            let _ = socket.send_to(&wire, peer);
        }
        ReplyTarget::Tcp { tx } => {
            let _ = tx.send(wire);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::forward::WaitCompletion;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::thread;
    use std::time::Instant;

    #[test]
    fn derive_shard_count_power_of_two_at_least_four() {
        assert_eq!(derive_shard_count(1), 4);
        assert_eq!(derive_shard_count(2), 4);
        assert_eq!(derive_shard_count(4), 4);
        assert_eq!(derive_shard_count(5), 8);
        assert_eq!(derive_shard_count(8), 8);
        assert_eq!(derive_shard_count(33), 64);
        assert_eq!(derive_shard_count(100), MAX_QUEUE_SHARDS);
        let q = PolicyQueue::with_shards(derive_shard_count(5));
        assert_eq!(q.shard_count(), 8);
        let r = ReplyRoutes::with_shards(derive_shard_count(5));
        assert_eq!(r.shard_count(), 8);
    }

    #[test]
    fn sticky_shard_same_for_new_resume_lookup() {
        let q = PolicyQueue::with_shards(8);
        let slot = SlotId::from_index(13);
        let new_shard = q.shard_for_slot(slot);
        assert_eq!(new_shard, 13 % 8);
        assert_eq!(q.shard_for_slot(PolicyWork::New(slot).slot_id()), new_shard);
        assert_eq!(
            q.shard_for_slot(PolicyWork::LookupResume(slot).slot_id()),
            new_shard
        );
        let resume = PolicyWork::Resume(IoResume {
            slot_id: slot,
            completion: WaitCompletion::Timeout,
        });
        assert_eq!(q.shard_for_slot(resume.slot_id()), new_shard);
    }

    #[test]
    fn reply_routes_same_shard_rule_as_queue() {
        let n = 8;
        let q = PolicyQueue::with_shards(n);
        let r = ReplyRoutes::with_shards(n);
        for i in 0..32u32 {
            let id = SlotId::from_index(i);
            assert_eq!(q.shard_for_slot(id), r.shard_for_slot(id));
        }
    }

    #[test]
    fn insert_take_pairing_across_shards() {
        let routes = ReplyRoutes::with_shards(4);
        let (tx, rx) = crossbeam_channel::bounded(1);
        let slot_a = SlotId::from_index(0); // shard 0
        let slot_b = SlotId::from_index(1); // shard 1
        routes.insert(slot_a, ReplyTarget::Tcp { tx: tx.clone() });
        routes.insert(slot_b, ReplyTarget::Tcp { tx });
        assert!(matches!(routes.take(slot_a), Some(ReplyTarget::Tcp { .. })));
        assert!(matches!(routes.take(slot_b), Some(ReplyTarget::Tcp { .. })));
        assert!(routes.take(slot_a).is_none());
        drop(rx);
    }

    #[test]
    fn concurrent_push_distinct_slots_while_one_shard_held() {
        // Hold shard 0; push for slot 1 (shard 1) must not need that lock.
        let q = Arc::new(PolicyQueue::with_shards(4));
        assert_eq!(q.shard_for_slot(SlotId::from_index(1)), 1);
        let done = Arc::new(AtomicBool::new(false));
        let q_push = q.clone();
        let done_push = done.clone();
        let joiner = thread::spawn(move || {
            q_push.push(PolicyWork::New(SlotId::from_index(1)));
            done_push.store(true, Ordering::SeqCst);
        });
        q.with_shard_locked(0, || {
            let start = Instant::now();
            while !done.load(Ordering::SeqCst) {
                if start.elapsed() > Duration::from_secs(2) {
                    panic!(
                        "push on distinct shard blocked while another shard held (global funnel?)"
                    );
                }
                thread::sleep(Duration::from_millis(1));
            }
        });
        joiner.join().unwrap();
        let shutdown = DataplaneShutdown::new();
        let work = q.pop(0, &shutdown).expect("work should be stealable");
        assert!(matches!(work, PolicyWork::New(id) if id == SlotId::from_index(1)));
    }

    #[test]
    fn concurrent_route_insert_while_other_shard_held() {
        let routes = Arc::new(ReplyRoutes::with_shards(4));
        let done = Arc::new(AtomicBool::new(false));
        let routes_push = routes.clone();
        let done_push = done.clone();
        let (tx, _rx) = crossbeam_channel::bounded::<Vec<u8>>(1);
        let joiner = thread::spawn(move || {
            routes_push.insert(SlotId::from_index(1), ReplyTarget::Tcp { tx });
            done_push.store(true, Ordering::SeqCst);
        });
        routes.with_shard_locked(0, || {
            let start = Instant::now();
            while !done.load(Ordering::SeqCst) {
                if start.elapsed() > Duration::from_secs(2) {
                    panic!("route insert on distinct shard blocked (global funnel?)");
                }
                thread::sleep(Duration::from_millis(1));
            }
        });
        joiner.join().unwrap();
        assert!(routes.take(SlotId::from_index(1)).is_some());
    }

    #[test]
    fn pop_steals_from_non_home_shard() {
        let q = PolicyQueue::with_shards(4);
        // Push onto shard 2; worker home is 0 must still obtain it.
        q.push(PolicyWork::New(SlotId::from_index(2)));
        let shutdown = DataplaneShutdown::new();
        let work = q.pop(0, &shutdown).expect("steal");
        assert!(matches!(work, PolicyWork::New(id) if id.index() == 2));
    }

    #[test]
    fn shutdown_stops_pop_without_stranding_check() {
        let q = PolicyQueue::with_shards(8);
        // Seed work on a high-index shard so home=0 must steal.
        for i in [3u32, 5, 7] {
            q.push(PolicyWork::New(SlotId::from_index(i)));
        }
        let shutdown = DataplaneShutdown::new();
        let mut got = 0;
        while q.pop(0, &shutdown).is_some() {
            got += 1;
            if got == 3 {
                break;
            }
        }
        assert_eq!(got, 3);
        shutdown.signal();
        assert!(q.pop(0, &shutdown).is_none());
    }
}
