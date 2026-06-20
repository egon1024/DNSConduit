//! Minimal transaction host for unit tests and local benchmarks.

use crate::host::{HostTransaction, ScriptPhase};
use std::collections::HashMap;
use std::net::{Ipv4Addr, Ipv6Addr};
use std::time::Instant;

pub struct MockHost {
    pub id: u64,
    pub qname: String,
    pub qtype: u16,
    pub qclass: u16,
    pub opcode: u8,
    pub edns_option_codes: Vec<u16>,
    pub dns_id: u16,
    pub rcode: Option<u16>,
    pub pool: Option<String>,
    pub retry: Option<String>,
    pub dropped: bool,
    pub soft_drop: bool,
    pub source_override_v4: Option<Ipv4Addr>,
    pub source_override_v6: Option<Ipv6Addr>,
    pub retry_source_override_v4: Option<Ipv4Addr>,
    pub retry_source_override_v6: Option<Ipv6Addr>,
    pub tags: HashMap<String, bool>,
    pub tag_strings: HashMap<String, String>,
    pub attempts: u32,
    pub started: Instant,
    pub last_forward_ms: u64,
    pub phase: ScriptPhase,
}

impl Default for MockHost {
    fn default() -> Self {
        Self {
            id: 1,
            qname: "test.example.".into(),
            qtype: 1,
            qclass: 1,
            opcode: 0,
            edns_option_codes: Vec::new(),
            dns_id: 1,
            rcode: None,
            pool: None,
            retry: None,
            dropped: false,
            soft_drop: false,
            source_override_v4: None,
            source_override_v6: None,
            retry_source_override_v4: None,
            retry_source_override_v6: None,
            tags: HashMap::new(),
            tag_strings: HashMap::new(),
            attempts: 0,
            started: Instant::now(),
            last_forward_ms: 0,
            phase: ScriptPhase::Request,
        }
    }
}

impl HostTransaction for MockHost {
    fn txn_id(&self) -> u64 {
        self.id
    }

    fn phase(&self) -> ScriptPhase {
        self.phase
    }

    fn question_qname(&self) -> Option<&str> {
        Some(&self.qname)
    }

    fn question_qtype(&self) -> Option<u16> {
        Some(self.qtype)
    }

    fn question_qclass(&self) -> Option<u16> {
        Some(self.qclass)
    }

    fn question_opcode(&self) -> Option<u8> {
        Some(self.opcode)
    }

    fn question_edns_option_codes(&self) -> &[u16] {
        &self.edns_option_codes
    }

    fn question_id(&self) -> u16 {
        self.dns_id
    }

    fn response_rcode_number(&self) -> Option<u16> {
        self.rcode
    }

    fn has_tag(&self, key: &str) -> bool {
        self.tags.get(key).copied().unwrap_or(false) || self.tag_strings.contains_key(key)
    }

    fn set_tag_bool(&mut self, key: &str, value: bool) {
        self.tags.insert(key.to_string(), value);
    }

    fn set_tag_string(&mut self, key: &str, value: &str) {
        self.tag_strings.insert(key.to_string(), value.to_string());
    }

    fn clear_tag(&mut self, key: &str) {
        self.tags.remove(key);
        self.tag_strings.remove(key);
    }

    fn set_pool(&mut self, name: &str) {
        self.pool = Some(name.to_string());
    }

    fn set_retry_pool(&mut self, name: &str) {
        self.retry = Some(name.to_string());
    }

    fn set_soft_drop(&mut self) {
        self.soft_drop = true;
    }

    fn clear_soft_drop(&mut self) {
        self.soft_drop = false;
        self.dropped = false;
    }

    fn clear_retry_pool(&mut self) {
        self.retry = None;
    }

    fn drop_query(&mut self) {
        self.set_soft_drop();
    }

    fn set_rcode_name(&mut self, name: &str) {
        self.rcode = crate::Rcode::parse_name(name).map(crate::Rcode::number);
    }

    fn set_rcode_number(&mut self, code: u16) {
        self.rcode = Some(code);
    }

    fn set_source_v4(&mut self, addr: &str) {
        if let Ok(ip) = addr.parse() {
            self.source_override_v4 = Some(ip);
        }
    }

    fn set_source_v6(&mut self, addr: &str) {
        if let Ok(ip) = addr.parse() {
            self.source_override_v6 = Some(ip);
        }
    }

    fn set_retry_source_v4(&mut self, addr: &str) {
        if let Ok(ip) = addr.parse() {
            self.retry_source_override_v4 = Some(ip);
        }
    }

    fn set_retry_source_v6(&mut self, addr: &str) {
        if let Ok(ip) = addr.parse() {
            self.retry_source_override_v6 = Some(ip);
        }
    }

    fn clear_retry_source_v4(&mut self) {
        self.retry_source_override_v4 = None;
    }

    fn clear_retry_source_v6(&mut self) {
        self.retry_source_override_v6 = None;
    }

    fn attempt_count(&self) -> u32 {
        self.attempts
    }

    fn started_at(&self) -> Instant {
        self.started
    }

    fn last_forward_ms(&self) -> u64 {
        self.last_forward_ms
    }

    fn is_dropped(&self) -> bool {
        self.dropped
    }

    fn mark_dropped(&mut self) {
        self.dropped = true;
    }

    fn script_tag_bools(&self) -> HashMap<String, bool> {
        self.tags.clone()
    }

    fn script_tag_strings(&self) -> HashMap<String, String> {
        self.tag_strings.clone()
    }
}
