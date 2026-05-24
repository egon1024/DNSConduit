//! Owned observation events queued for sink consumers.

use std::net::SocketAddr;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventKind {
    Query,
    Response,
    Retry,
}

#[derive(Debug, Clone)]
pub struct ObservationEvent {
    pub kind: EventKind,
    pub txn_id: u64,
    pub client_addr: SocketAddr,
    pub protocol_udp: bool,
    pub wire: Vec<u8>,
    pub attempt_count: u32,
    pub extra: Option<Vec<u8>>,
}
