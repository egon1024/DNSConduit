//! `HostTransaction` adapter for `Transaction`.

use crate::phase::Phase;
use crate::transaction::Transaction;
use conduit_script::{HostTransaction, ScriptPhase};
use std::collections::HashMap;

impl HostTransaction for Transaction {
    fn txn_id(&self) -> u64 {
        self.id
    }

    fn phase(&self) -> ScriptPhase {
        match self.current_phase {
            Phase::ResponseRules | Phase::WaitResponse | Phase::Send => ScriptPhase::Response,
            _ => ScriptPhase::Request,
        }
    }

    fn question_qname(&self) -> Option<&str> {
        self.qname.as_deref()
    }

    fn question_qtype_label(&self) -> Option<String> {
        self.qtype_label()
    }

    fn question_id(&self) -> u16 {
        self.dns_id
    }

    fn response_rcode_label(&self) -> Option<String> {
        self.rcode_label()
    }

    fn has_tag(&self, key: &str) -> bool {
        self.tags.has(key)
    }

    fn set_tag_bool(&mut self, key: &str, value: bool) {
        self.tags.set_bool(key, value);
    }

    fn set_tag_string(&mut self, key: &str, value: &str) {
        self.tags.set_string(key, value);
    }

    fn set_pool(&mut self, name: &str) {
        self.selected_pool = Some(name.to_string());
    }

    fn set_retry_pool(&mut self, name: &str) {
        self.retry_pool = Some(name.to_string());
    }

    fn set_soft_drop(&mut self) {
        self.soft_drop = true;
    }

    fn clear_soft_drop(&mut self) {
        self.soft_drop = false;
        self.dropped = false;
    }

    fn clear_retry_pool(&mut self) {
        self.retry_pool = None;
    }

    fn drop_query(&mut self) {
        self.set_soft_drop();
    }

    fn set_rcode_name(&mut self, name: &str) {
        Transaction::set_rcode_name(self, name);
    }

    fn set_source_v4(&mut self, addr: &str) {
        if let Ok(ip) = addr.parse() {
            self.set_source_override_v4(ip);
        }
    }

    fn set_source_v6(&mut self, addr: &str) {
        if let Ok(ip) = addr.parse() {
            self.set_source_override_v6(ip);
        }
    }

    fn attempt_count(&self) -> u32 {
        self.attempt_count
    }

    fn started_at(&self) -> std::time::Instant {
        self.started_at
    }

    fn is_dropped(&self) -> bool {
        self.dropped
    }

    fn mark_dropped(&mut self) {
        self.dropped = true;
    }

    fn script_tag_bools(&self) -> HashMap<String, bool> {
        self.tags.bool_flags().clone()
    }
}
