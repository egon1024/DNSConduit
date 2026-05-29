//! Outstanding upstream query ID mapping.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Mutex;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ForwardKey {
    pub backend: SocketAddr,
    pub dns_id: u16,
}

pub struct TxnTable {
    capacity: usize,
    per_backend_limit: u32,
    entries: Mutex<HashMap<ForwardKey, u64>>,
    per_backend: Mutex<HashMap<SocketAddr, u32>>,
}

impl TxnTable {
    pub fn new(capacity: usize, per_backend_limit: u32) -> Self {
        Self {
            capacity,
            per_backend_limit,
            entries: Mutex::new(HashMap::new()),
            per_backend: Mutex::new(HashMap::new()),
        }
    }

    pub fn register(&self, key: ForwardKey, txn_id: u64) -> bool {
        let mut entries = self.entries.lock().unwrap();
        if entries.len() >= self.capacity {
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
}
