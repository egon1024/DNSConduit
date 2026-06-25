//! Preallocated transaction slot pool for dataplane runtimes.

mod wire_buf;

use crate::transaction::{ClientProtocol, Transaction};
use std::net::SocketAddr;
use std::sync::atomic::{AtomicU64, Ordering};
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

/// Chunked slot arena with free list and exhaustion accounting.
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

/// Process-wide slot pool (mutex-backed for multi-threaded ingress workers).
#[derive(Clone)]
pub struct SharedTxnStore {
    inner: std::sync::Arc<std::sync::Mutex<TxnStore>>,
}

impl SharedTxnStore {
    pub fn new(capacity: u32, chunk_size: u32) -> Self {
        Self {
            inner: std::sync::Arc::new(std::sync::Mutex::new(TxnStore::new(capacity, chunk_size))),
        }
    }

    pub fn lock(&self) -> std::sync::MutexGuard<'_, TxnStore> {
        self.inner.lock().expect("txn store mutex poisoned")
    }

    pub fn capacity(&self) -> u32 {
        self.lock().capacity()
    }

    pub fn in_use(&self) -> u32 {
        self.lock().in_use()
    }

    pub fn exhaustion_total(&self) -> u64 {
        self.lock().exhaustion_total()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
