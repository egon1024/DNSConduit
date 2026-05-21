//! Per-query state carried through pipeline phases (spec §4.1).

use crate::phase::Phase;
use crate::routing::AttemptRecord;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::time::Instant;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClientProtocol {
    Udp,
    Tcp,
}

#[derive(Debug, Default, Clone)]
pub struct TagSet {
    flags: HashMap<String, bool>,
    strings: HashMap<String, String>,
}

impl TagSet {
    pub fn set_bool(&mut self, key: impl Into<String>, value: bool) {
        self.flags.insert(key.into(), value);
    }

    pub fn set_string(&mut self, key: impl Into<String>, value: impl Into<String>) {
        self.strings.insert(key.into(), value.into());
    }

    pub fn has(&self, key: &str) -> bool {
        self.flags.get(key).copied().unwrap_or(false) || self.strings.contains_key(key)
    }
}

pub struct Transaction {
    pub id: u64,
    pub tags: TagSet,
    pub current_phase: Phase,
    pub query_wire: Vec<u8>,
    pub response_wire: Option<Vec<u8>>,
    pub dns_id: u16,
    pub qname: Option<String>,
    pub qtype: Option<u16>,
    pub client_addr: SocketAddr,
    pub protocol: ClientProtocol,
    pub client_udp_payload_size: Option<u16>,
    pub selected_pool: Option<String>,
    pub selected_backend: Option<SocketAddr>,
    pub attempts: Vec<AttemptRecord>,
    pub attempt_count: u32,
    pub retry_pool: Option<String>,
    pub started_at: Instant,
    pub snapshot_generation: u64,
    rcode: Option<u16>,
}

impl Transaction {
    pub fn new(id: u64, client_addr: SocketAddr, protocol: ClientProtocol) -> Self {
        Self {
            id,
            tags: TagSet::default(),
            current_phase: Phase::Receive,
            query_wire: Vec::new(),
            response_wire: None,
            dns_id: 0,
            qname: None,
            qtype: None,
            client_addr,
            protocol,
            client_udp_payload_size: None,
            selected_pool: None,
            selected_backend: None,
            attempts: Vec::new(),
            attempt_count: 0,
            retry_pool: None,
            started_at: Instant::now(),
            snapshot_generation: 0,
            rcode: None,
        }
    }

    pub fn with_query_wire(mut self, wire: Vec<u8>) -> Self {
        self.query_wire = wire;
        self
    }

    pub fn qtype_label(&self) -> Option<String> {
        self.qtype.map(|t| match t {
            1 => "A".into(),
            28 => "AAAA".into(),
            _ => format!("TYPE{t}"),
        })
    }

    pub fn rcode_label(&self) -> Option<String> {
        self.rcode.map(|r| match r {
            0 => "NOERROR".into(),
            2 => "SERVFAIL".into(),
            3 => "NXDOMAIN".into(),
            5 => "REFUSED".into(),
            _ => format!("RCODE{r}"),
        })
    }

    pub fn set_rcode_name(&mut self, name: &str) {
        self.rcode = Some(match name.to_uppercase().as_str() {
            "NOERROR" => 0,
            "SERVFAIL" => 2,
            "NXDOMAIN" => 3,
            "REFUSED" => 5,
            "FORMERR" => 1,
            _ => 2,
        });
    }

    pub fn set_rcode(&mut self, code: u16) {
        self.rcode = Some(code);
    }

    pub fn rcode(&self) -> Option<u16> {
        self.rcode
    }

    pub fn record_attempt(&mut self, pool: String, backend: SocketAddr) {
        self.attempt_count += 1;
        self.attempts.push(AttemptRecord {
            pool: pool.clone(),
            backend,
            attempt: self.attempt_count,
        });
        self.selected_pool = Some(pool);
        self.selected_backend = Some(backend);
    }

    pub fn take_retry_pool(&mut self) -> Option<String> {
        self.retry_pool.take()
    }
}
