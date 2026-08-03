//! Outstanding upstream query ID mapping.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicU16, Ordering};
use std::sync::Mutex;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ForwardKey {
    pub backend: SocketAddr,
    pub dns_id: u16,
}

/// Rewrite the DNS message ID in the first two bytes of a wire buffer.
pub fn rewrite_dns_id(wire: &mut [u8], id: u16) {
    if wire.len() >= 2 {
        let bytes = id.to_be_bytes();
        wire[0] = bytes[0];
        wire[1] = bytes[1];
    }
}

pub struct TxnTable {
    capacity: usize,
    per_backend_limit: u32,
    entries: Mutex<HashMap<ForwardKey, u64>>,
    per_backend: Mutex<HashMap<SocketAddr, u32>>,
    next_dns_id: AtomicU16,
}

impl TxnTable {
    pub fn new(capacity: usize, per_backend_limit: u32) -> Self {
        Self {
            capacity,
            per_backend_limit,
            entries: Mutex::new(HashMap::new()),
            per_backend: Mutex::new(HashMap::new()),
            next_dns_id: AtomicU16::new(0),
        }
    }

    /// Register an explicit upstream key. Refuses duplicates (never overwrites).
    pub fn register(&self, key: ForwardKey, txn_id: u64) -> bool {
        let mut entries = self.entries.lock().unwrap();
        if entries.len() >= self.capacity {
            return false;
        }
        if entries.contains_key(&key) {
            return false;
        }
        let mut counts = self.per_backend.lock().unwrap();
        let count = counts.entry(key.backend).or_insert(0);
        if *count >= self.per_backend_limit {
            return false;
        }
        *count += 1;
        entries.insert(key, txn_id);
        true
    }

    /// Allocate a free upstream DNS ID for `backend` and register it.
    ///
    /// Client query IDs are not unique under multi-client load; demux keys must
    /// be unique per outstanding forward to the same backend.
    pub fn register_unique(&self, backend: SocketAddr, txn_id: u64) -> Option<ForwardKey> {
        let mut entries = self.entries.lock().unwrap();
        if entries.len() >= self.capacity {
            return None;
        }
        let mut counts = self.per_backend.lock().unwrap();
        let count = counts.entry(backend).or_insert(0);
        if *count >= self.per_backend_limit {
            return None;
        }
        let start = self.next_dns_id.fetch_add(1, Ordering::Relaxed);
        for offset in 0..=u16::MAX {
            let dns_id = start.wrapping_add(offset);
            let key = ForwardKey { backend, dns_id };
            if entries.contains_key(&key) {
                continue;
            }
            *count += 1;
            entries.insert(key, txn_id);
            return Some(key);
        }
        None
    }

    pub fn remove(&self, key: ForwardKey) {
        let mut entries = self.entries.lock().unwrap();
        if entries.remove(&key).is_some() {
            let mut counts = self.per_backend.lock().unwrap();
            if let Some(c) = counts.get_mut(&key.backend) {
                *c = c.saturating_sub(1);
            }
        }
    }

    pub fn lookup(&self, key: ForwardKey) -> Option<u64> {
        self.entries.lock().unwrap().get(&key).copied()
    }

    /// Current outstanding forward count per backend address (for metrics scrape).
    pub fn outstanding_per_backend(&self) -> Vec<(SocketAddr, u32)> {
        self.per_backend
            .lock()
            .unwrap()
            .iter()
            .filter(|(_, c)| **c > 0)
            .map(|(addr, c)| (*addr, *c))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enforces_per_backend_limit() {
        let table = TxnTable::new(100, 1);
        let backend: SocketAddr = "127.0.0.1:5300".parse().unwrap();
        let k1 = ForwardKey { backend, dns_id: 1 };
        let k2 = ForwardKey { backend, dns_id: 2 };
        assert!(table.register(k1, 10));
        assert!(!table.register(k2, 11));
        table.remove(k1);
        assert!(table.register(k2, 11));
    }

    #[test]
    fn register_refuses_duplicate_key() {
        let table = TxnTable::new(100, 10);
        let backend: SocketAddr = "127.0.0.1:5300".parse().unwrap();
        let key = ForwardKey { backend, dns_id: 7 };
        assert!(table.register(key, 1));
        // Duplicate must not displace the original mapping.
        assert!(!table.register(key, 2));
        assert_eq!(table.lookup(key), Some(1));
    }

    #[test]
    fn register_unique_avoids_colliding_ids() {
        let table = TxnTable::new(100, 64);
        let backend: SocketAddr = "127.0.0.1:5300".parse().unwrap();
        let a = table.register_unique(backend, 1).expect("first");
        let b = table.register_unique(backend, 2).expect("second");
        assert_ne!(a.dns_id, b.dns_id);
        assert_eq!(table.lookup(a), Some(1));
        assert_eq!(table.lookup(b), Some(2));
    }

    #[test]
    fn rewrite_dns_id_sets_header_bytes() {
        let mut wire = vec![0x12, 0x34, 0x01, 0x00];
        rewrite_dns_id(&mut wire, 0xabcd);
        assert_eq!(&wire[..2], &[0xab, 0xcd]);
    }
}
