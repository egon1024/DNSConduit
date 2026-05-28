//! Per-query state carried through pipeline phases (spec §4.1).

use crate::phase::Phase;
use crate::routing::AttemptRecord;
use conduit_config::forward::RecursionDesired;
use conduit_metrics::TraceLog;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::time::Instant;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClientProtocol {
    Udp,
    Tcp,
}

pub type ExportedTagBools = Vec<(String, bool)>;
pub type ExportedTagStrings = Vec<(String, String)>;

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

    pub fn bool_flags(&self) -> &HashMap<String, bool> {
        &self.flags
    }

    /// Export tags for observation `extra` (all bool flags with value true, all string tags).
    pub fn export_all_tags(&self) -> (ExportedTagBools, ExportedTagStrings) {
        let bools: ExportedTagBools = self.flags.iter().map(|(k, v)| (k.clone(), *v)).collect();
        let strings: ExportedTagStrings = self
            .strings
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        (bools, strings)
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
    /// Configured listener bind address (used as `listener` metric label).
    pub listener_label: Option<String>,
    pub protocol: ClientProtocol,
    pub client_udp_payload_size: Option<u16>,
    pub selected_pool: Option<String>,
    pub selected_backend: Option<SocketAddr>,
    pub attempts: Vec<AttemptRecord>,
    pub attempt_count: u32,
    pub retry_pool: Option<String>,
    pub started_at: Instant,
    pub snapshot_generation: u64,
    pub dropped: bool,
    /// Rhai/script override for upstream RD bit (`set_rd` / `clear_rd`).
    pub rd_override: Option<bool>,
    /// Rhai/script override for IPv4 egress source (`set_source_v4`).
    pub source_override_v4: Option<std::net::Ipv4Addr>,
    /// Rhai/script override for IPv6 egress source (`set_source_v6`).
    pub source_override_v6: Option<std::net::Ipv6Addr>,
    /// Pipeline trace buffer; `None` when tracing is off for this transaction.
    pub trace_log: Option<TraceLog>,
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
            listener_label: None,
            protocol,
            client_udp_payload_size: None,
            selected_pool: None,
            selected_backend: None,
            attempts: Vec::new(),
            attempt_count: 0,
            retry_pool: None,
            started_at: Instant::now(),
            snapshot_generation: 0,
            dropped: false,
            rd_override: None,
            source_override_v4: None,
            source_override_v6: None,
            trace_log: None,
            rcode: None,
        }
    }

    pub fn trace_record_phase(
        &mut self,
        phase: &str,
        message: Option<String>,
        pool: Option<String>,
        backend: Option<String>,
    ) {
        if let Some(log) = self.trace_log.as_mut() {
            log.record(phase, self.started_at, message, pool, backend);
        }
    }

    pub fn set_source_override_v4(&mut self, addr: std::net::Ipv4Addr) {
        self.source_override_v4 = Some(addr);
    }

    pub fn set_source_override_v6(&mut self, addr: std::net::Ipv6Addr) {
        self.source_override_v6 = Some(addr);
    }

    pub fn set_rd_override(&mut self, rd: bool) {
        self.rd_override = Some(rd);
    }

    pub fn clear_rd_override(&mut self) {
        self.rd_override = Some(false);
    }

    /// Upstream RD policy: Rhai override when set, otherwise preserve client RD.
    pub fn upstream_rd_policy(&self) -> RecursionDesired {
        match self.rd_override {
            Some(true) => RecursionDesired::Set,
            Some(false) => RecursionDesired::Clear,
            None => RecursionDesired::Preserve,
        }
    }

    pub fn with_query_wire(mut self, wire: Vec<u8>) -> Self {
        self.query_wire = wire;
        self
    }

    pub fn with_listener_label(mut self, label: impl Into<String>) -> Self {
        self.listener_label = Some(label.into());
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
