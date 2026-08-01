//! Preallocated transaction slot pool for dataplane runtimes.
//!
//! # Locking model (`SharedTxnStore`)
//!
//! The shared pool uses two lock kinds:
//!
//! 1. **Free-list / meta lock** — short-held only. Covers the free list, chunk
//!    growth of the slot arena, and capacity/accounting reads that need a
//!    consistent free-list view.
//! 2. **Per-slot lock** — exclusive access to one [`TxnSlot`]. Policy may hold
//!    this lock across orchestrator work and the Policy→IoWait transition so a
//!    Resume for the **same** slot cannot race ahead of IoWait publish.
//!
//! ## Lock order (deadlock avoidance)
//!
//! - **Never** take the free-list / meta lock while holding a slot lock.
//! - **Never** acquire two slot locks at once (no cross-slot nest).
//! - Taking a slot lock while briefly holding the meta lock is allowed only for
//!   acquire/grow paths that need the slot's `Arc` before releasing meta; prefer
//!   cloning the slot `Arc` under meta, dropping meta, then locking the slot.
//!
//! Call sites that previously held a process-wide store mutex across an
//! orchestrator body must use [`SharedTxnStore::with_slot_exclusive`] (or
//! equivalent per-slot helpers) so distinct slots can progress concurrently.

mod wire_buf;

use crate::transaction::{ClientProtocol, Transaction};
use std::net::SocketAddr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
pub use wire_buf::{WireBuffer, WireBufferError, DNS_WIRE_BUFFER_SIZE};

pub const DEFAULT_SLOT_CHUNK_SIZE: u32 = 256;

/// Handle to a slot in the pool (stable while the slot exists).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SlotId(u32);

impl SlotId {
    pub fn index(self) -> u32 {
        self.0
    }

    pub const fn from_index(index: u32) -> Self {
        Self(index)
    }
}

/// Explicit slot lifecycle (see dataplane-runtime-models design §3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SlotState {
    Free,
    Ingress,
    Policy,
    IoWait,
    SidecarWait,
    ReplyPending,
    IngressSend,
    Terminal,
}

/// One transaction slot: orchestrator state plus fixed wire buffers.
pub struct TxnSlot {
    pub state: SlotState,
    pub txn: Transaction,
    pub query: WireBuffer,
    pub response: WireBuffer,
    /// Rare large TCP payloads that exceed [`DNS_WIRE_BUFFER_SIZE`].
    pub response_overflow: Option<Vec<u8>>,
    pub pre_parsed: bool,
}

impl TxnSlot {
    fn new_free() -> Self {
        Self {
            state: SlotState::Free,
            txn: Transaction::new(
                0,
                "0.0.0.0:0".parse::<SocketAddr>().expect("placeholder"),
                ClientProtocol::Udp,
            ),
            query: WireBuffer::default(),
            response: WireBuffer::default(),
            response_overflow: None,
            pre_parsed: false,
        }
    }

    fn clear(&mut self) {
        self.state = SlotState::Free;
        self.txn = Transaction::new(
            0,
            "0.0.0.0:0".parse().expect("placeholder"),
            ClientProtocol::Udp,
        );
        self.query.clear();
        self.response.clear();
        self.response_overflow = None;
        self.pre_parsed = false;
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AcquireError {
    Exhausted,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SlotError {
    InvalidId(SlotId),
    StateMismatch {
        expected: SlotState,
        actual: SlotState,
    },
    NotTerminal(SlotState),
}

/// Unsynchronized chunked slot arena (unit tests and single-threaded helpers).
pub struct TxnStore {
    slots: Vec<TxnSlot>,
    free_list: Vec<SlotId>,
    capacity: u32,
    chunk_size: u32,
    exhaustion_total: AtomicU64,
}

impl TxnStore {
    pub fn new(capacity: u32, chunk_size: u32) -> Self {
        assert!(capacity >= 1, "txn store capacity must be >= 1");
        assert!(chunk_size >= 1, "txn store chunk_size must be >= 1");
        Self {
            slots: Vec::new(),
            free_list: Vec::new(),
            capacity,
            chunk_size,
            exhaustion_total: AtomicU64::new(0),
        }
    }

    /// Configured maximum slots (`orchestrator.txn_table_capacity`).
    pub fn capacity(&self) -> u32 {
        self.capacity
    }

    /// Slots allocated so far (grows in chunks up to [`Self::capacity`]).
    pub fn allocated(&self) -> u32 {
        self.slots.len() as u32
    }

    /// Slots not on the free list.
    pub fn in_use(&self) -> u32 {
        self.allocated().saturating_sub(self.free_list.len() as u32)
    }

    /// Count non-`Free` slots matching `pred` (drain enumeration; includes parked `IoWait`).
    pub fn active_slots_matching<F>(&self, pred: F) -> u32
    where
        F: Fn(&TxnSlot) -> bool,
    {
        self.slots
            .iter()
            .filter(|slot| slot.state != SlotState::Free && pred(slot))
            .count() as u32
    }

    /// Cumulative acquire failures when at capacity (`conduit_slot_pool_exhausted_total` hook).
    pub fn exhaustion_total(&self) -> u64 {
        self.exhaustion_total.load(Ordering::Relaxed)
    }

    /// Acquire a free slot, growing the pool by one chunk when needed.
    pub fn acquire(&mut self) -> Result<SlotId, AcquireError> {
        if self.free_list.is_empty() && !self.grow_chunk() {
            self.exhaustion_total.fetch_add(1, Ordering::Relaxed);
            return Err(AcquireError::Exhausted);
        }
        let id = self.free_list.pop().expect("free slot after grow");
        let slot = self.slot_mut(id).expect("id from free list must exist");
        slot.state = SlotState::Ingress;
        slot.txn.id = id.index() as u64;
        Ok(id)
    }

    /// Return a terminal slot to the free list.
    pub fn release(&mut self, id: SlotId) -> Result<(), SlotError> {
        self.with_slot(id, SlotState::Terminal, |slot| {
            slot.clear();
            Ok(())
        })?;
        self.free_list.push(id);
        Ok(())
    }

    /// Exclusive access while the slot is in `expected` state.
    pub fn with_slot<R, F>(&mut self, id: SlotId, expected: SlotState, f: F) -> Result<R, SlotError>
    where
        F: FnOnce(&mut TxnSlot) -> Result<R, SlotError>,
    {
        let slot = self.slot_mut(id).ok_or(SlotError::InvalidId(id))?;
        if slot.state != expected {
            return Err(SlotError::StateMismatch {
                expected,
                actual: slot.state,
            });
        }
        f(slot)
    }

    /// Transition `from` → `to` when the caller holds the expected state lock implicitly.
    pub fn transition(
        &mut self,
        id: SlotId,
        from: SlotState,
        to: SlotState,
    ) -> Result<(), SlotError> {
        self.with_slot(id, from, |slot| {
            slot.state = to;
            Ok(())
        })
    }

    fn grow_chunk(&mut self) -> bool {
        let allocated = self.slots.len() as u32;
        if allocated >= self.capacity {
            return false;
        }
        let grow_by = self.chunk_size.min(self.capacity - allocated);
        let base = allocated;
        self.slots.reserve(grow_by as usize);
        for i in 0..grow_by {
            self.slots.push(TxnSlot::new_free());
            self.free_list.push(SlotId(base + i));
        }
        true
    }

    fn slot_mut(&mut self, id: SlotId) -> Option<&mut TxnSlot> {
        self.slots.get_mut(id.0 as usize)
    }

    /// Return an active slot to the free list (sync path and error cleanup).
    pub fn release_active(&mut self, id: SlotId) -> Result<(), SlotError> {
        let slot = self.slot_mut(id).ok_or(SlotError::InvalidId(id))?;
        if slot.state == SlotState::Free {
            return Ok(());
        }
        slot.clear();
        self.free_list.push(id);
        Ok(())
    }
}

struct SharedTxnMeta {
    slots: Vec<Arc<Mutex<TxnSlot>>>,
    free_list: Vec<SlotId>,
}

struct SharedTxnInner {
    /// Free-list / chunk-growth lock — short-held only (see module docs).
    meta: Mutex<SharedTxnMeta>,
    capacity: u32,
    chunk_size: u32,
    exhaustion_total: AtomicU64,
}

/// Process-wide slot pool with per-slot mutual exclusion.
///
/// Distinct slots can run policy/orchestrator work concurrently; each slot still
/// has exclusive access under its own mutex. Free-list operations use a separate,
/// short-held lock (see module-level lock order).
#[derive(Clone)]
pub struct SharedTxnStore {
    inner: Arc<SharedTxnInner>,
}

impl SharedTxnStore {
    pub fn new(capacity: u32, chunk_size: u32) -> Self {
        assert!(capacity >= 1, "txn store capacity must be >= 1");
        assert!(chunk_size >= 1, "txn store chunk_size must be >= 1");
        Self {
            inner: Arc::new(SharedTxnInner {
                meta: Mutex::new(SharedTxnMeta {
                    slots: Vec::new(),
                    free_list: Vec::new(),
                }),
                capacity,
                chunk_size,
                exhaustion_total: AtomicU64::new(0),
            }),
        }
    }

    pub fn capacity(&self) -> u32 {
        self.inner.capacity
    }

    pub fn in_use(&self) -> u32 {
        let meta = self.meta_lock();
        let allocated = meta.slots.len() as u32;
        allocated.saturating_sub(meta.free_list.len() as u32)
    }

    pub fn allocated(&self) -> u32 {
        self.meta_lock().slots.len() as u32
    }

    /// Count non-`Free` slots matching `pred` (drain enumeration; includes parked `IoWait`).
    ///
    /// Deadlock-free: copies slot Arcs under the meta lock, then locks each slot
    /// individually without re-taking meta (and never two slot locks at once).
    pub fn active_slots_matching<F>(&self, pred: F) -> u32
    where
        F: Fn(&TxnSlot) -> bool,
    {
        let arcs: Vec<Arc<Mutex<TxnSlot>>> = {
            let meta = self.meta_lock();
            meta.slots.to_vec()
        };
        let mut count = 0u32;
        for arc in arcs {
            let slot = arc.lock().expect("slot mutex poisoned");
            if slot.state != SlotState::Free && pred(&slot) {
                count += 1;
            }
        }
        count
    }

    pub fn exhaustion_total(&self) -> u64 {
        self.inner.exhaustion_total.load(Ordering::Relaxed)
    }

    /// Acquire a free slot, growing the pool by one chunk when needed.
    pub fn acquire(&self) -> Result<SlotId, AcquireError> {
        let arc = {
            let mut meta = self.meta_lock();
            if meta.free_list.is_empty()
                && !Self::grow_chunk(&mut meta, self.inner.capacity, self.inner.chunk_size)
            {
                self.inner.exhaustion_total.fetch_add(1, Ordering::Relaxed);
                return Err(AcquireError::Exhausted);
            }
            let id = meta.free_list.pop().expect("free slot after grow");
            let arc = meta
                .slots
                .get(id.index() as usize)
                .cloned()
                .expect("id from free list must exist");
            // Drop meta before locking the slot (lock order).
            (id, arc)
        };
        let (id, arc) = arc;
        {
            let mut slot = arc.lock().expect("slot mutex poisoned");
            slot.state = SlotState::Ingress;
            slot.txn.id = id.index() as u64;
        }
        Ok(id)
    }

    /// Return a terminal slot to the free list.
    pub fn release(&self, id: SlotId) -> Result<(), SlotError> {
        self.with_slot(id, SlotState::Terminal, |slot| {
            slot.clear();
            Ok(())
        })?;
        self.return_to_free(id);
        Ok(())
    }

    /// Exclusive access while the slot is in `expected` state.
    pub fn with_slot<R, F>(&self, id: SlotId, expected: SlotState, f: F) -> Result<R, SlotError>
    where
        F: FnOnce(&mut TxnSlot) -> Result<R, SlotError>,
    {
        self.with_slot_exclusive(id, |slot| {
            if slot.state != expected {
                return Err(SlotError::StateMismatch {
                    expected,
                    actual: slot.state,
                });
            }
            f(slot)
        })
    }

    /// Hold this slot's mutex for the duration of `f` (orchestrator + state
    /// transitions for a single slot). Does not take the free-list lock.
    pub fn with_slot_exclusive<R, F>(&self, id: SlotId, f: F) -> Result<R, SlotError>
    where
        F: FnOnce(&mut TxnSlot) -> Result<R, SlotError>,
    {
        let arc = self.slot_arc(id)?;
        let mut guard = arc.lock().expect("slot mutex poisoned");
        f(&mut guard)
    }

    /// Transition `from` → `to` under the slot's mutex.
    pub fn transition(&self, id: SlotId, from: SlotState, to: SlotState) -> Result<(), SlotError> {
        self.with_slot(id, from, |slot| {
            slot.state = to;
            Ok(())
        })
    }

    /// Return an active slot to the free list (sync path and error cleanup).
    pub fn release_active(&self, id: SlotId) -> Result<(), SlotError> {
        let should_return = self.with_slot_exclusive(id, |slot| {
            if slot.state == SlotState::Free {
                Ok(false)
            } else {
                slot.clear();
                Ok(true)
            }
        })?;
        if should_return {
            self.return_to_free(id);
        }
        Ok(())
    }

    fn slot_arc(&self, id: SlotId) -> Result<Arc<Mutex<TxnSlot>>, SlotError> {
        let meta = self.meta_lock();
        meta.slots
            .get(id.index() as usize)
            .cloned()
            .ok_or(SlotError::InvalidId(id))
    }

    fn return_to_free(&self, id: SlotId) {
        let mut meta = self.meta_lock();
        meta.free_list.push(id);
    }

    fn meta_lock(&self) -> MutexGuard<'_, SharedTxnMeta> {
        self.inner
            .meta
            .lock()
            .expect("txn store meta mutex poisoned")
    }

    fn grow_chunk(meta: &mut SharedTxnMeta, capacity: u32, chunk_size: u32) -> bool {
        let allocated = meta.slots.len() as u32;
        if allocated >= capacity {
            return false;
        }
        let grow_by = chunk_size.min(capacity - allocated);
        let base = allocated;
        meta.slots.reserve(grow_by as usize);
        for i in 0..grow_by {
            meta.slots.push(Arc::new(Mutex::new(TxnSlot::new_free())));
            meta.free_list.push(SlotId(base + i));
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Barrier;
    use std::thread;
    use std::time::{Duration, Instant};

    #[test]
    fn acquire_and_release_round_trip() {
        let mut store = TxnStore::new(4, 2);
        assert_eq!(store.allocated(), 0);
        let id = store.acquire().unwrap();
        assert_eq!(store.in_use(), 1);
        assert_eq!(store.allocated(), 2);
        store
            .transition(id, SlotState::Ingress, SlotState::Policy)
            .unwrap();
        store
            .transition(id, SlotState::Policy, SlotState::Terminal)
            .unwrap();
        store.release(id).unwrap();
        assert_eq!(store.in_use(), 0);
        assert_eq!(store.free_list.len(), 2);
    }

    #[test]
    fn chunk_growth_up_to_capacity() {
        let mut store = TxnStore::new(5, 2);
        let mut ids = Vec::new();
        for _ in 0..5 {
            ids.push(store.acquire().unwrap());
        }
        assert_eq!(store.allocated(), 5);
        assert_eq!(store.in_use(), 5);
        for id in ids {
            store
                .transition(id, SlotState::Ingress, SlotState::Terminal)
                .unwrap();
            store.release(id).unwrap();
        }
    }

    #[test]
    fn exhaustion_at_capacity() {
        let mut store = TxnStore::new(2, 2);
        let _a = store.acquire().unwrap();
        let _b = store.acquire().unwrap();
        assert_eq!(store.acquire(), Err(AcquireError::Exhausted));
        assert_eq!(store.exhaustion_total(), 1);
        assert_eq!(store.acquire(), Err(AcquireError::Exhausted));
        assert_eq!(store.exhaustion_total(), 2);
    }

    #[test]
    fn state_mismatch_rejected() {
        let mut store = TxnStore::new(4, 4);
        let id = store.acquire().unwrap();
        assert_eq!(
            store.with_slot(id, SlotState::Policy, |_| Ok(())),
            Err(SlotError::StateMismatch {
                expected: SlotState::Policy,
                actual: SlotState::Ingress,
            })
        );
    }

    #[test]
    fn release_requires_terminal() {
        let mut store = TxnStore::new(4, 4);
        let id = store.acquire().unwrap();
        assert_eq!(
            store.release(id),
            Err(SlotError::StateMismatch {
                expected: SlotState::Terminal,
                actual: SlotState::Ingress,
            })
        );
    }

    #[test]
    fn wire_buffer_holds_typical_udp_query() {
        let mut store = TxnStore::new(1, 1);
        let id = store.acquire().unwrap();
        store
            .with_slot(id, SlotState::Ingress, |slot| {
                slot.query.set_from_slice(&[0u8; 512]).unwrap();
                Ok(())
            })
            .unwrap();
        let slot = store.slot_mut(id).unwrap();
        assert_eq!(slot.query.len(), 512);
    }

    #[test]
    fn wire_buffer_rejects_oversize() {
        let mut buf = WireBuffer::default();
        let data = vec![0u8; DNS_WIRE_BUFFER_SIZE + 1];
        assert_eq!(
            buf.set_from_slice(&data),
            Err(WireBufferError::TooLarge {
                len: DNS_WIRE_BUFFER_SIZE + 1,
                max: DNS_WIRE_BUFFER_SIZE,
            })
        );
    }

    #[test]
    fn shared_acquire_release_round_trip() {
        let store = SharedTxnStore::new(4, 2);
        assert_eq!(store.allocated(), 0);
        let id = store.acquire().unwrap();
        assert_eq!(store.in_use(), 1);
        assert_eq!(store.allocated(), 2);
        store
            .transition(id, SlotState::Ingress, SlotState::Policy)
            .unwrap();
        store
            .transition(id, SlotState::Policy, SlotState::Terminal)
            .unwrap();
        store.release(id).unwrap();
        assert_eq!(store.in_use(), 0);
    }

    #[test]
    fn shared_concurrent_distinct_slots_progress() {
        let store = SharedTxnStore::new(8, 4);
        let a = store.acquire().unwrap();
        let b = store.acquire().unwrap();
        store
            .transition(a, SlotState::Ingress, SlotState::Policy)
            .unwrap();
        store
            .transition(b, SlotState::Ingress, SlotState::Policy)
            .unwrap();

        let barrier = Arc::new(Barrier::new(2));
        let hold = Duration::from_millis(80);
        let started = Instant::now();

        let t1 = {
            let store = store.clone();
            let barrier = Arc::clone(&barrier);
            thread::spawn(move || {
                store
                    .with_slot_exclusive(a, |slot| {
                        assert_eq!(slot.state, SlotState::Policy);
                        barrier.wait();
                        thread::sleep(hold);
                        slot.state = SlotState::IoWait;
                        Ok(())
                    })
                    .unwrap();
            })
        };
        let t2 = {
            let store = store.clone();
            let barrier = Arc::clone(&barrier);
            thread::spawn(move || {
                store
                    .with_slot_exclusive(b, |slot| {
                        assert_eq!(slot.state, SlotState::Policy);
                        barrier.wait();
                        thread::sleep(hold);
                        slot.state = SlotState::IoWait;
                        Ok(())
                    })
                    .unwrap();
            })
        };
        t1.join().unwrap();
        t2.join().unwrap();

        // If a process-wide lock covered both exclusive sections, elapsed would
        // be ~2*hold. Concurrent per-slot locks should finish near one hold.
        let elapsed = started.elapsed();
        assert!(
            elapsed < hold + Duration::from_millis(50),
            "distinct slots must not serialize on a process-wide lock; elapsed {elapsed:?}"
        );

        store
            .with_slot(a, SlotState::IoWait, |slot| {
                slot.state = SlotState::Terminal;
                Ok(())
            })
            .unwrap();
        store
            .with_slot(b, SlotState::IoWait, |slot| {
                slot.state = SlotState::Terminal;
                Ok(())
            })
            .unwrap();
        store.release(a).unwrap();
        store.release(b).unwrap();
    }

    #[test]
    fn shared_active_slots_matching_under_concurrent_mutate() {
        let store = SharedTxnStore::new(16, 4);
        let ids: Vec<_> = (0..4).map(|_| store.acquire().unwrap()).collect();
        for &id in &ids {
            store
                .transition(id, SlotState::Ingress, SlotState::Policy)
                .unwrap();
        }

        let store2 = store.clone();
        let mutator = thread::spawn(move || {
            for _ in 0..200 {
                let count = store2.active_slots_matching(|_| true);
                assert!(count <= 4);
                thread::yield_now();
            }
        });

        for &id in &ids {
            store
                .transition(id, SlotState::Policy, SlotState::IoWait)
                .unwrap();
            store
                .transition(id, SlotState::IoWait, SlotState::Terminal)
                .unwrap();
            store.release(id).unwrap();
        }
        mutator.join().unwrap();
        assert_eq!(store.active_slots_matching(|_| true), 0);
    }
}
