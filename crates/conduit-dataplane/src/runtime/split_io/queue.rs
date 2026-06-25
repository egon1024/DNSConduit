//! Work queues and reply routing for split_io.

use crate::forward::IoResume;
use conduit_core::txn_store::SlotId;
use std::collections::{HashMap, VecDeque};
use std::net::{SocketAddr, UdpSocket};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

use crate::listener::DataplaneShutdown;

/// Policy pool work item.
#[derive(Debug, Clone)]
pub enum PolicyWork {
    New(SlotId),
    Resume(IoResume),
}

pub struct PolicyQueue {
    inner: Mutex<VecDeque<PolicyWork>>,
    cv: Condvar,
}

impl PolicyQueue {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(VecDeque::new()),
            cv: Condvar::new(),
        }
    }

    pub fn push(&self, work: PolicyWork) {
        self.inner.lock().unwrap().push_back(work);
        self.cv.notify_one();
    }

    pub fn pop(&self, shutdown: &DataplaneShutdown) -> Option<PolicyWork> {
        let mut guard = self.inner.lock().unwrap();
        loop {
            if shutdown.is_shutdown() {
                return None;
            }
            if let Some(work) = guard.pop_front() {
                return Some(work);
            }
            guard = self
                .cv
                .wait_timeout(guard, Duration::from_millis(100))
                .unwrap()
                .0;
        }
    }
}

/// Where to deliver the client response for a slot.
#[derive(Debug)]
pub enum ReplyTarget {
    Udp {
        socket: Arc<UdpSocket>,
        peer: SocketAddr,
    },
    Tcp {
        tx: crossbeam_channel::Sender<Vec<u8>>,
    },
}

pub struct ReplyRoutes {
    inner: Mutex<HashMap<SlotId, ReplyTarget>>,
}

impl ReplyRoutes {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(HashMap::new()),
        }
    }

    pub fn insert(&self, slot_id: SlotId, target: ReplyTarget) {
        self.inner.lock().unwrap().insert(slot_id, target);
    }

    pub fn take(&self, slot_id: SlotId) -> Option<ReplyTarget> {
        self.inner.lock().unwrap().remove(&slot_id)
    }
}

pub fn deliver_reply(routes: &ReplyRoutes, slot_id: SlotId, wire: Vec<u8>) {
    let Some(target) = routes.take(slot_id) else {
        return;
    };
    match target {
        ReplyTarget::Udp { socket, peer } => {
            let _ = socket.send_to(&wire, peer);
        }
        ReplyTarget::Tcp { tx } => {
            let _ = tx.send(wire);
        }
    }
}
