//! Per-query state carried through pipeline phases (spec §4.1).

use crate::phase::Phase;
use std::collections::HashMap;

#[derive(Debug, Default, Clone)]
pub struct TagSet {
    // Phase 3: typed tag map (bool, i64, string, blob)
    flags: HashMap<String, bool>,
}

impl TagSet {
    pub fn set_bool(&mut self, key: impl Into<String>, value: bool) {
        self.flags.insert(key.into(), value);
    }

    pub fn has(&self, key: &str) -> bool {
        self.flags.get(key).copied().unwrap_or(false)
    }
}

pub struct Transaction {
    pub id: u64,
    pub tags: TagSet,
    pub current_phase: Phase,
}

impl Transaction {
    pub fn new(id: u64) -> Self {
        Self {
            id,
            tags: TagSet::default(),
            current_phase: Phase::Receive,
        }
    }
}
