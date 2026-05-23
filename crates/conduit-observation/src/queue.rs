//! Bounded per-sink queue with drop policy.

use crate::event::ObservationEvent;
use crossbeam_channel::{bounded, Receiver, Sender, TrySendError};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DropPolicy {
    DropOldest,
    DropNewest,
}

impl DropPolicy {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "drop_oldest" => Some(Self::DropOldest),
            "drop_newest" => Some(Self::DropNewest),
            _ => None,
        }
    }
}

pub struct SinkQueue {
    tx: Sender<ObservationEvent>,
    rx: Receiver<ObservationEvent>,
    policy: DropPolicy,
}

impl SinkQueue {
    pub fn new(capacity: usize, policy: DropPolicy) -> Self {
        let (tx, rx) = bounded(capacity);
        Self { tx, rx, policy }
    }

    pub fn sender(&self) -> Sender<ObservationEvent> {
        self.tx.clone()
    }

    pub fn receiver(&self) -> Receiver<ObservationEvent> {
        self.rx.clone()
    }

    /// Enqueue without blocking. Returns `true` if an event was dropped.
    pub fn try_enqueue(&self, event: ObservationEvent) -> bool {
        match self.tx.try_send(event) {
            Ok(()) => false,
            Err(TrySendError::Full(event)) => match self.policy {
                DropPolicy::DropNewest => true,
                DropPolicy::DropOldest => {
                    while self.rx.try_recv().is_ok() {}
                    let _ = self.tx.try_send(event);
                    true
                }
            },
            Err(TrySendError::Disconnected(_)) => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::{EventKind, ObservationEvent};
    use std::net::{IpAddr, Ipv4Addr, SocketAddr};

    fn sample_event(id: u64) -> ObservationEvent {
        ObservationEvent {
            kind: EventKind::Query,
            txn_id: id,
            client_addr: SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 53),
            protocol_udp: true,
            wire: vec![id as u8],
            attempt_count: 1,
        }
    }

    #[test]
    fn drop_newest_discards_incoming() {
        let q = SinkQueue::new(1, DropPolicy::DropNewest);
        assert!(!q.try_enqueue(sample_event(1)));
        assert!(q.try_enqueue(sample_event(2)));
        assert_eq!(q.receiver().try_recv().unwrap().txn_id, 1);
        assert!(q.receiver().try_recv().is_err());
    }

    #[test]
    fn drop_oldest_evicts_oldest() {
        let q = SinkQueue::new(1, DropPolicy::DropOldest);
        assert!(!q.try_enqueue(sample_event(1)));
        assert!(q.try_enqueue(sample_event(2)));
        assert_eq!(q.receiver().try_recv().unwrap().txn_id, 2);
    }
}
