//! Borrowed transaction view at enqueue sites.

use std::net::SocketAddr;

/// Narrow view passed from dataplane workers; avoids coupling to `conduit-core::Transaction`.
#[derive(Debug, Clone)]
pub struct TxnView<'a> {
    pub txn_id: u64,
    pub global_query_index: u64,
    pub client_addr: SocketAddr,
    pub protocol_udp: bool,
    pub qname: Option<&'a str>,
    pub qtype_label: Option<String>,
    pub query_wire: &'a [u8],
    pub response_wire: Option<&'a [u8]>,
    pub attempt_count: u32,
    pub extra: TxnExtraSource,
}

/// Owned metadata for optional `Dnstap.extra` JSON (built per sink at enqueue).
#[derive(Debug, Clone, Default)]
pub struct TxnExtraSource {
    pub pool: Option<String>,
    pub backend: Option<String>,
    pub attempt_count: u32,
    pub txn_id: u64,
    pub qname: Option<String>,
    pub rcode_label: Option<String>,
    pub client: String,
    pub tag_bools: Vec<(String, bool)>,
    pub tag_strings: Vec<(String, String)>,
}
