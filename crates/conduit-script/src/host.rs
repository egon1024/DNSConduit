use std::time::Instant;

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
    fn response_rcode_number(&self) -> Option<u16> {
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
