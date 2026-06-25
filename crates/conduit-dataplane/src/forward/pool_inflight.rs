//! Per-pool concurrent forward limits (`pools[].max_inflight`).

use conduit_proto::config::Config;
use std::collections::HashMap;
use std::sync::Mutex;

pub struct PoolInflight {
    limits: HashMap<String, u32>,
    active: Mutex<HashMap<String, u32>>,
}

impl PoolInflight {
    pub fn from_config(cfg: &Config) -> Self {
        let limits = cfg
            .pools
            .iter()
            .filter_map(|p| p.max_inflight.map(|n| (p.name.clone(), n)))
            .collect();
        Self {
            limits,
            active: Mutex::new(HashMap::new()),
        }
    }

    /// Reserve one in-flight slot for `pool` when configured. Returns false when at cap.
    pub fn try_acquire(&self, pool: &str) -> bool {
        let Some(limit) = self.limits.get(pool).copied() else {
            return true;
        };
        let mut active = self.active.lock().unwrap();
        let count = active.entry(pool.to_string()).or_insert(0);
        if *count >= limit {
            return false;
        }
        *count += 1;
        true
    }

    pub fn release(&self, pool: &str) {
        if !self.limits.contains_key(pool) {
            return;
        }
        let mut active = self.active.lock().unwrap();
        if let Some(count) = active.get_mut(pool) {
            *count = count.saturating_sub(1);
        }
    }
}
