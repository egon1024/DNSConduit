use std::net::SocketAddr;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ResponseWireMeta {
    pub answer_count: u16,
    pub authority_count: u16,
    pub additional_count: u16,
    pub truncated: bool,
    pub authoritative: bool,
}

/// Client transport for the query (mirrors `conduit-core::ClientProtocol`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClientProtocol {
    Udp,
    Tcp,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScriptPhase {
    Request,
    Response,
}

/// Host transaction surface for Rhai scripts (implemented by `conduit-core::Transaction`).
pub trait HostTransaction {
    fn txn_id(&self) -> u64;
    /// Process-wide query index (YAML `every_nth_global`); default `0` when unset.
    fn global_query_index(&self) -> u64 {
        0
    }
    /// Config snapshot generation active when this transaction started.
    fn snapshot_generation(&self) -> u64 {
        0
    }
    fn phase(&self) -> ScriptPhase;
    fn question_qname(&self) -> Option<&str>;
    fn question_qtype(&self) -> Option<u16>;
    fn question_qclass(&self) -> Option<u16> {
        None
    }
    fn question_opcode(&self) -> Option<u8> {
        None
    }
    fn question_edns_option_codes(&self) -> &[u16] {
        &[]
    }
    fn question_id(&self) -> u16;
    fn client_addr(&self) -> SocketAddr;
    fn client_protocol(&self) -> ClientProtocol;
    fn listener_label(&self) -> Option<&str> {
        None
    }
    fn client_udp_payload_size(&self) -> Option<u16> {
        None
    }
    fn received_at(&self) -> SystemTime {
        SystemTime::UNIX_EPOCH
    }
    fn selected_pool(&self) -> Option<&str> {
        None
    }
    fn selected_backend(&self) -> Option<SocketAddr> {
        None
    }
    /// Logical label for the selected backend: configured `name` when set, else address.
    fn selected_backend_label(&self) -> Option<String> {
        None
    }
    fn response_rcode_number(&self) -> Option<u16> {
        None
    }
    fn response_meta(&self) -> Option<ResponseWireMeta> {
        None
    }
    fn has_tag(&self, key: &str) -> bool;
    fn set_tag_bool(&mut self, key: &str, value: bool);
    fn set_tag_string(&mut self, key: &str, value: &str);
    fn clear_tag(&mut self, key: &str);
    fn set_pool(&mut self, name: &str);
    fn set_retry_pool(&mut self, name: &str);
    fn set_soft_drop(&mut self);
    fn clear_soft_drop(&mut self);
    fn clear_retry_pool(&mut self);
    fn drop_query(&mut self);
    fn set_rcode_name(&mut self, name: &str);
    fn set_rcode_number(&mut self, code: u16) {
        let _ = code;
    }
    fn set_source_v4(&mut self, addr: &str);
    fn set_source_v6(&mut self, addr: &str);
    fn set_retry_source_v4(&mut self, addr: &str);
    fn set_retry_source_v6(&mut self, addr: &str);
    fn clear_retry_source_v4(&mut self);
    fn clear_retry_source_v6(&mut self);
    fn attempt_count(&self) -> u32;
    fn started_at(&self) -> Instant;
    fn last_forward_ms(&self) -> u64;
    fn is_dropped(&self) -> bool;
    fn mark_dropped(&mut self);
    /// Bool tags on the host transaction at hook entry (for `has_tag` in scripts).
    fn script_tag_bools(&self) -> std::collections::HashMap<String, bool> {
        std::collections::HashMap::new()
    }
    /// String tags on the host transaction at hook entry (for `has_tag` in scripts).
    fn script_tag_strings(&self) -> std::collections::HashMap<String, String> {
        std::collections::HashMap::new()
    }
}

/// Seconds since Unix epoch (UTC) for wall-clock helpers.
pub fn unix_secs(time: SystemTime) -> u64 {
    time.duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// UTC hour (0–23) and ISO weekday (Monday = 1 … Sunday = 7) from Unix seconds.
pub fn utc_hour_and_weekday(unix_secs: u64) -> (u8, u8) {
    const SECS_PER_DAY: u64 = 86_400;
    let days = unix_secs / SECS_PER_DAY;
    let hour = ((unix_secs % SECS_PER_DAY) / 3600) as u8;
    // 1970-01-01 was a Thursday (ISO weekday 4).
    let weekday = ((days + 3) % 7 + 1) as u8;
    (hour, weekday)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utc_hour_weekday_epoch_thursday() {
        let (hour, wd) = utc_hour_and_weekday(0);
        assert_eq!(hour, 0);
        assert_eq!(wd, 4);
    }
}
