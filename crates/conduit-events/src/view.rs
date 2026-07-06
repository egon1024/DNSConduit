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
    pub qtype: Option<u16>,
    pub rcode: Option<u16>,
    pub qclass: Option<u16>,
    pub opcode: Option<u8>,
    pub edns_option_codes: &'a [u16],
    /// Human-readable qtype for dnstap extra export (not used for selector matching).
    pub qtype_label: Option<String>,
    pub query_wire: &'a [u8],
    pub response_wire: Option<&'a [u8]>,
    pub attempt_count: u32,
    /// `cache` or `forward` when a response is available for filtering.
    pub answer_source: Option<&'a str>,
    /// Named cache instance when the answer came from cache.
    pub cache_instance: Option<&'a str>,
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
    pub answer_source: Option<String>,
    pub cache_instance: Option<String>,
    pub tag_bools: Vec<(String, bool)>,
    pub tag_strings: Vec<(String, String)>,
}
