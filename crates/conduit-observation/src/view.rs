//! Borrowed transaction view at enqueue sites.

use std::net::SocketAddr;

/// Narrow view passed from dataplane workers; avoids coupling to `conduit-core::Transaction`.
#[derive(Debug, Clone)]
pub struct TxnView<'a> {
    pub txn_id: u64,
    pub client_addr: SocketAddr,
    pub protocol_udp: bool,
    pub qname: Option<&'a str>,
    pub query_wire: &'a [u8],
    pub response_wire: Option<&'a [u8]>,
    pub attempt_count: u32,
}
