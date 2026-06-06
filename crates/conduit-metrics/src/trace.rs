//! In-memory pipeline trace buffer and store (design §4.3).

use parking_lot::Mutex;
use std::collections::VecDeque;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, serde::Serialize)]
pub struct TraceEvent {
    pub phase: String,
    pub elapsed_us: u64,
    pub message: Option<String>,
    pub pool: Option<String>,
    pub backend: Option<String>,
}

#[derive(Debug, Default)]
pub struct TraceLog {
    pub events: Vec<TraceEvent>,
}

impl TraceLog {
    pub fn record(
        &mut self,
        phase: &str,
        started_at: Instant,
        message: Option<String>,
        pool: Option<String>,
        backend: Option<String>,
    ) {
        let elapsed_us = started_at.elapsed().as_micros() as u64;
        self.events.push(TraceEvent {
            phase: phase.to_string(),
            elapsed_us,
            message,
            pool,
            backend,
        });
    }
}

struct StoredTrace {
    events: Vec<TraceEvent>,
    inserted_at: Instant,
}

pub struct TraceStore {
    max_entries: usize,
    ttl: Duration,
    entries: Mutex<VecDeque<(String, StoredTrace)>>,
}

impl TraceStore {
    pub fn new(max_entries: usize, ttl: Duration) -> Self {
        Self {
            max_entries,
            ttl,
            entries: Mutex::new(VecDeque::new()),
        }
    }

    pub fn insert(&self, txn_id: u64, events: Vec<TraceEvent>) {
        if events.is_empty() {
            return;
        }
        let key = txn_id.to_string();
        let mut q = self.entries.lock();
        q.retain(|(_, t)| t.inserted_at.elapsed() < self.ttl);
        while q.len() >= self.max_entries {
            q.pop_front();
        }
        q.push_back((
            key,
            StoredTrace {
                events,
                inserted_at: Instant::now(),
            },
        ));
    }

    pub fn get(&self, txn_id: &str) -> Option<Vec<TraceEvent>> {
        let mut q = self.entries.lock();
        q.retain(|(_, t)| t.inserted_at.elapsed() < self.ttl);
        q.iter()
            .find(|(k, _)| k == txn_id)
            .map(|(_, t)| t.events.clone())
    }
}
