use std::time::Instant;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScriptPhase {
    Request,
    Response,
}

/// Host transaction surface for Rhai scripts (implemented by `conduit-core::Transaction`).
pub trait HostTransaction {
    fn txn_id(&self) -> u64;
    fn phase(&self) -> ScriptPhase;
    fn question_qname(&self) -> Option<&str>;
    fn question_qtype_label(&self) -> Option<String>;
    fn question_id(&self) -> u16;
    fn response_rcode_label(&self) -> Option<String>;
    fn has_tag(&self, key: &str) -> bool;
    fn set_tag_bool(&mut self, key: &str, value: bool);
    fn set_tag_string(&mut self, key: &str, value: &str);
    fn set_pool(&mut self, name: &str);
    fn set_retry_pool(&mut self, name: &str);
    fn drop_query(&mut self);
    fn set_rcode_name(&mut self, name: &str);
    fn set_rd(&mut self, value: bool);
    fn clear_rd(&mut self);
    fn set_source_v4(&mut self, addr: &str);
    fn set_source_v6(&mut self, addr: &str);
    fn attempt_count(&self) -> u32;
    fn started_at(&self) -> Instant;
    fn is_dropped(&self) -> bool;
    fn mark_dropped(&mut self);
    /// Bool tags on the host transaction at hook entry (for `has_tag` in scripts).
    fn script_tag_bools(&self) -> std::collections::HashMap<String, bool> {
        std::collections::HashMap::new()
    }
}
