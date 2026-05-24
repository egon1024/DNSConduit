//! Per-sink observation counters (in-process; phase 4 exports to Prometheus).

use crate::event::EventKind;
use std::sync::atomic::{AtomicU64, AtomicU8, Ordering};
use std::sync::Arc;

/// Snapshot of one sink's counters (cheap clone for operators / future scrape).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SinkMetricsSnapshot {
    pub name: String,
    pub enqueued_query: u64,
    pub enqueued_response: u64,
    pub enqueued_retry: u64,
    pub queue_dropped: u64,
    pub delivered: u64,
    pub write_failed: u64,
    pub encode_failed: u64,
    pub connect_attempts: u64,
    pub connected: u8,
}

/// Per-sink atomic counters; shared by hub (enqueue) and dnstap worker (delivery).
#[derive(Debug)]
pub struct SinkMetrics {
    name: String,
    enqueued_query: AtomicU64,
    enqueued_response: AtomicU64,
    enqueued_retry: AtomicU64,
    queue_dropped: AtomicU64,
    delivered: AtomicU64,
    write_failed: AtomicU64,
    encode_failed: AtomicU64,
    connect_attempts: AtomicU64,
    connected: AtomicU8,
}

impl SinkMetrics {
    pub fn new(name: String) -> Arc<Self> {
        Arc::new(Self {
            name,
            enqueued_query: AtomicU64::new(0),
            enqueued_response: AtomicU64::new(0),
            enqueued_retry: AtomicU64::new(0),
            queue_dropped: AtomicU64::new(0),
            delivered: AtomicU64::new(0),
            write_failed: AtomicU64::new(0),
            encode_failed: AtomicU64::new(0),
            connect_attempts: AtomicU64::new(0),
            connected: AtomicU8::new(0),
        })
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn record_enqueued(&self, kind: EventKind) {
        let counter = match kind {
            EventKind::Query => &self.enqueued_query,
            EventKind::Response => &self.enqueued_response,
            EventKind::Retry => &self.enqueued_retry,
        };
        counter.fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_queue_dropped(&self) {
        self.queue_dropped.fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_delivered(&self) {
        self.delivered.fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_write_failed(&self) {
        self.write_failed.fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_encode_failed(&self) {
        self.encode_failed.fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_connect_attempt(&self) {
        self.connect_attempts.fetch_add(1, Ordering::Relaxed);
    }

    pub fn set_connected(&self, connected: bool) {
        self.connected
            .store(u8::from(connected), Ordering::Relaxed);
    }

    pub fn snapshot(&self) -> SinkMetricsSnapshot {
        SinkMetricsSnapshot {
            name: self.name.clone(),
            enqueued_query: self.enqueued_query.load(Ordering::Relaxed),
            enqueued_response: self.enqueued_response.load(Ordering::Relaxed),
            enqueued_retry: self.enqueued_retry.load(Ordering::Relaxed),
            queue_dropped: self.queue_dropped.load(Ordering::Relaxed),
            delivered: self.delivered.load(Ordering::Relaxed),
            write_failed: self.write_failed.load(Ordering::Relaxed),
            encode_failed: self.encode_failed.load(Ordering::Relaxed),
            connect_attempts: self.connect_attempts.load(Ordering::Relaxed),
            connected: self.connected.load(Ordering::Relaxed),
        }
    }

    pub fn queue_dropped_total(&self) -> u64 {
        self.queue_dropped.load(Ordering::Relaxed)
    }
}
