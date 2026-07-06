//! Single-flight coalescing per (cache instance, key).

use super::key::CacheKey;
use parking_lot::{Condvar, Mutex};
use std::sync::Arc;
use std::time::Duration;

/// Result of registering for in-flight coalescing.
#[derive(Debug, PartialEq, Eq)]
pub enum InFlightRole {
    /// This transaction should proceed to upstream (forward provider).
    Leader,
    /// Another transaction is in-flight; wait for its result.
    Follower,
}

struct InFlightInner {
    leader_active: bool,
    result: Option<Arc<[u8]>>,
    waiters: Vec<u64>,
}

pub struct InFlightGate {
    inner: Mutex<InFlightInner>,
    cv: Condvar,
}

impl InFlightGate {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(InFlightInner {
                leader_active: false,
                result: None,
                waiters: Vec::new(),
            }),
            cv: Condvar::new(),
        }
    }

    pub fn register(&self, txn_id: u64, async_wait: bool) -> InFlightRole {
        let mut guard = self.inner.lock();
        if guard.leader_active {
            if async_wait {
                guard.waiters.push(txn_id);
            }
            return InFlightRole::Follower;
        }
        // Start a new flight (prior flight completed or this is the first attempt).
        guard.result = None;
        guard.waiters.clear();
        guard.leader_active = true;
        InFlightRole::Leader
    }

    /// Block until the leader completes (sync runtime path).
    pub fn wait_for_result(&self, timeout: Duration) -> Option<Arc<[u8]>> {
        let mut guard = self.inner.lock();
        let deadline = std::time::Instant::now() + timeout;
        while guard.result.is_none() && guard.leader_active {
            let remaining = deadline.saturating_duration_since(std::time::Instant::now());
            if remaining.is_zero() {
                return None;
            }
            if self.cv.wait_for(&mut guard, remaining).timed_out() {
                return None;
            }
        }
        guard.result.clone()
    }

    pub fn complete(&self, wire: Option<Arc<[u8]>>) -> Vec<u64> {
        let mut guard = self.inner.lock();
        guard.result = wire;
        guard.leader_active = false;
        let waiters = std::mem::take(&mut guard.waiters);
        self.cv.notify_all();
        waiters
    }

    pub fn has_result(&self) -> bool {
        self.inner.lock().result.is_some()
    }

    pub fn result(&self) -> Option<Arc<[u8]>> {
        self.inner.lock().result.clone()
    }

    pub fn reset(&self) {
        let mut guard = self.inner.lock();
        *guard = InFlightInner {
            leader_active: false,
            result: None,
            waiters: Vec::new(),
        };
    }
}

pub struct InFlightTable {
    flights: Mutex<std::collections::HashMap<CacheKey, Arc<InFlightGate>>>,
}

impl Default for InFlightTable {
    fn default() -> Self {
        Self::new()
    }
}

impl InFlightTable {
    pub fn new() -> Self {
        Self {
            flights: Mutex::new(std::collections::HashMap::new()),
        }
    }

    pub fn gate_for(&self, key: &CacheKey) -> Arc<InFlightGate> {
        let mut guard = self.flights.lock();
        guard
            .entry(key.clone())
            .or_insert_with(|| Arc::new(InFlightGate::new()))
            .clone()
    }

    pub fn remove(&self, key: &CacheKey) {
        self.flights.lock().remove(key);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;

    #[test]
    fn single_flight_coalescing() {
        let table = InFlightTable::new();
        let key = CacheKey(b"coalesce".to_vec());
        let gate = table.gate_for(&key);

        assert_eq!(gate.register(1, false), InFlightRole::Leader);
        assert_eq!(gate.register(2, true), InFlightRole::Follower);

        let gate2 = table.gate_for(&key);
        let handle = thread::spawn(move || gate2.wait_for_result(Duration::from_secs(2)));

        thread::sleep(Duration::from_millis(20));
        let wire: Arc<[u8]> = Arc::from(b"answer".as_slice());
        let waiters = gate.complete(Some(wire.clone()));
        assert_eq!(waiters, vec![2]);

        assert_eq!(handle.join().unwrap(), Some(wire));
    }
}
