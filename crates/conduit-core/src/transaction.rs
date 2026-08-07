//! Per-query state carried through pipeline phases (spec §4.1).

use crate::lookup::{AnswerSource, LookupForwardStep, LookupOutcome};
use crate::parse_reject::ParseRejectReason;
use crate::phase::Phase;
use crate::routing::AttemptRecord;
use conduit_metrics::{MetricsPin, TraceLog};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Instant, SystemTime};

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

    pub fn clear(&mut self, key: &str) {
        self.flags.remove(key);
        self.strings.remove(key);
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
    pub global_query_index: u64,
    pub tags: TagSet,
    pub current_phase: Phase,
    pub query_wire: Vec<u8>,
    pub response_wire: Option<Vec<u8>>,
    /// Parsed upstream header/section metadata when compile-time gating enables wire parsing.
    pub response_meta: Option<conduit_script::ResponseWireMeta>,
    pub dns_id: u16,
    pub qname: Option<String>,
    pub qtype: Option<u16>,
    pub qclass: Option<u16>,
    /// DNS opcode from the query header (0–15).
    pub opcode: Option<u8>,
    /// EDNS option codes present on the client query (empty when no OPT).
    pub edns_option_codes: Vec<u16>,
    pub client_addr: SocketAddr,
    /// Set when parse stage drops the query (for metrics).
    pub parse_reject_reason: Option<ParseRejectReason>,
    /// Configured listener bind address (used as `listener` metric label).
    pub listener_label: Option<String>,
    pub protocol: ClientProtocol,
    pub client_udp_payload_size: Option<u16>,
    /// Set when Send clips outbound UDP wire to client payload size and sets TC=1.
    pub udp_response_truncated_on_send: bool,
    pub selected_pool: Option<String>,
    pub selected_backend: Option<SocketAddr>,
    /// Logical label for the selected backend: configured `name` when set, else address.
    /// Resolved once at selection time so traces, events, logs, and Rhai agree.
    pub selected_backend_label: Option<String>,
    pub attempts: Vec<AttemptRecord>,
    pub attempt_count: u32,
    pub retry_pool: Option<String>,
    pub started_at: Instant,
    /// Wall-clock time when the transaction was created (UTC anchor for Rhai `now_unix`).
    pub received_at: SystemTime,
    /// Upstream forward RTT in milliseconds for the most recent forward attempt (`0` before any attempt completes).
    pub last_forward_ms: u64,
    /// Start time of the in-flight forward attempt; set at send, cleared when RTT is recorded (`split_io` park/resume).
    pub forward_started_at: Option<Instant>,
    pub snapshot_generation: u64,
    /// Metrics registries pinned at orchestrator start for this txn (Gate G4).
    /// Keeps recording on the generation that started the query across plan swaps.
    pub metrics_pin: Option<MetricsPin>,
    pub dropped: bool,
    /// Soft drop intent from `drop` / `drop_query()`; resolved at end of the current rule.
    pub soft_drop: bool,
    /// Rhai/script override for IPv4 egress source (`set_source_v4`).
    pub source_override_v4: Option<std::net::Ipv4Addr>,
    /// Rhai/script override for IPv6 egress source (`set_source_v6`).
    pub source_override_v6: Option<std::net::Ipv6Addr>,
    /// One-shot IPv4 egress for the next retry forward (`set_retry_source_v4`); ignored when `attempt_count <= 1` at Forward.
    pub retry_source_override_v4: Option<std::net::Ipv4Addr>,
    /// One-shot IPv6 egress for the next retry forward (`set_retry_source_v6`); ignored when `attempt_count <= 1` at Forward.
    pub retry_source_override_v6: Option<std::net::Ipv6Addr>,
    /// Pipeline trace buffer; `None` when tracing is off for this transaction.
    pub trace_log: Option<TraceLog>,
    /// Ingress structural parse already populated metadata; skip wire parse in orchestrator.
    pub pre_parsed: bool,
    /// Wall time when the pipeline suspended (split_io I/O park); used for Lookup phase metrics.
    pub suspend_phase_started_at: Option<Instant>,
    /// Set when sync forward (or submit resume) already recorded conduit_forward_* metrics.
    pub forward_metrics_recorded: bool,
    /// Active lookup profile name (default when unset).
    pub lookup_profile: Option<String>,
    /// Outcome of the most recent lookup provider attempt.
    pub lookup_outcome: Option<LookupOutcome>,
    /// How the answer was produced (`cache` or `forward`).
    pub answer_source: Option<AnswerSource>,
    /// Named cache instance when `answer_source` is cache (pipeline state, not an operator tag).
    pub cache_instance: Option<String>,
    /// When false, cache lookup provider returns Bypass.
    pub cache_lookup_eligible: bool,
    /// Resume point inside forward provider after async upstream I/O.
    pub lookup_forward_step: Option<LookupForwardStep>,
    /// Pending cache single-flight coalesce (split_io async wait).
    pub lookup_cache_wait: Option<LookupCacheWait>,
    /// Cache fill target after forward completes (leader single-flight).
    pub lookup_cache_fill: Option<LookupCacheFill>,
    rcode: Option<u16>,
}

/// Resume point after cache single-flight wait.
#[derive(Debug, Clone)]
pub struct LookupCacheWait {
    pub cache_name: String,
    pub key: Vec<u8>,
}

/// Cache instance + key to fill when forward returns an answer.
#[derive(Debug, Clone)]
pub struct LookupCacheFill {
    pub cache_name: String,
    pub key: Vec<u8>,
}

impl Transaction {
    pub fn new(id: u64, client_addr: SocketAddr, protocol: ClientProtocol) -> Self {
        Self {
            id,
            global_query_index: 0,
            tags: TagSet::default(),
            current_phase: Phase::Receive,
            query_wire: Vec::new(),
            response_wire: None,
            response_meta: None,
            dns_id: 0,
            qname: None,
            qtype: None,
            qclass: None,
            opcode: None,
            edns_option_codes: Vec::new(),
            client_addr,
            parse_reject_reason: None,
            listener_label: None,
            protocol,
            client_udp_payload_size: None,
            udp_response_truncated_on_send: false,
            selected_pool: None,
            selected_backend: None,
            selected_backend_label: None,
            attempts: Vec::new(),
            attempt_count: 0,
            retry_pool: None,
            started_at: Instant::now(),
            received_at: SystemTime::now(),
            last_forward_ms: 0,
            forward_started_at: None,
            snapshot_generation: 0,
            metrics_pin: None,
            dropped: false,
            soft_drop: false,
            source_override_v4: None,
            source_override_v6: None,
            retry_source_override_v4: None,
            retry_source_override_v6: None,
            trace_log: None,
            pre_parsed: false,
            suspend_phase_started_at: None,
            forward_metrics_recorded: false,
            lookup_profile: None,
            lookup_outcome: None,
            answer_source: None,
            cache_instance: None,
            cache_lookup_eligible: true,
            lookup_forward_step: None,
            lookup_cache_wait: None,
            lookup_cache_fill: None,
            rcode: None,
        }
    }

    /// Builtin registry for this txn: pinned generation when set, else the hub's current.
    pub fn builtin_registry(
        &self,
        hub: &conduit_metrics::MetricsHub,
    ) -> Arc<conduit_metrics::BuiltinRegistry> {
        self.metrics_pin
            .as_ref()
            .map(|p| Arc::clone(&p.builtin))
            .unwrap_or_else(|| hub.builtin())
    }

    pub fn trace_record_phase(
        &mut self,
        phase: &str,
        message: Option<String>,
        pool: Option<String>,
        backend: Option<String>,
        cache: Option<String>,
    ) {
        if let Some(log) = self.trace_log.as_mut() {
            log.record(phase, self.started_at, message, pool, backend, cache);
        }
    }

    /// Mark the start of an upstream forward attempt (send). Used by `split_io` to measure RTT across park/resume.
    pub fn mark_forward_started(&mut self, at: Instant) {
        self.forward_started_at = Some(at);
    }

    /// Record upstream RTT for the most recent forward attempt and clear `forward_started_at`.
    pub fn complete_forward_rtt(&mut self, started: Instant) {
        self.last_forward_ms = forward_elapsed_ms(started);
        self.forward_started_at = None;
    }

    /// Record upstream RTT from `forward_started_at` (I/O backend completion in `split_io`).
    pub fn complete_forward_rtt_from_mark(&mut self) {
        if let Some(started) = self.forward_started_at.take() {
            self.last_forward_ms = forward_elapsed_ms(started);
        }
    }

    pub fn last_forward_ms(&self) -> u64 {
        self.last_forward_ms
    }

    pub fn set_source_override_v4(&mut self, addr: std::net::Ipv4Addr) {
        self.source_override_v4 = Some(addr);
    }

    pub fn set_source_override_v6(&mut self, addr: std::net::Ipv6Addr) {
        self.source_override_v6 = Some(addr);
    }

    pub fn set_retry_source_override_v4(&mut self, addr: std::net::Ipv4Addr) {
        self.retry_source_override_v4 = Some(addr);
    }

    pub fn set_retry_source_override_v6(&mut self, addr: std::net::Ipv6Addr) {
        self.retry_source_override_v6 = Some(addr);
    }

    pub fn clear_retry_source_override_v4(&mut self) {
        self.retry_source_override_v4 = None;
    }

    pub fn clear_retry_source_override_v6(&mut self) {
        self.retry_source_override_v6 = None;
    }

    /// IPv4 egress override at [Forward](crate::phase::Phase::Forward): one-shot `retry_source_override_v4` on retry forwards (`attempt_count > 1`), else `source_override_v4`.
    pub fn take_effective_source_override_v4(&mut self) -> Option<std::net::Ipv4Addr> {
        if self.attempt_count > 1 {
            if let Some(addr) = self.retry_source_override_v4.take() {
                return Some(addr);
            }
        }
        self.source_override_v4
    }

    /// IPv6 egress override at Forward: one-shot `retry_source_override_v6` on retry forwards (`attempt_count > 1`), else `source_override_v6`.
    pub fn take_effective_source_override_v6(&mut self) -> Option<std::net::Ipv6Addr> {
        if self.attempt_count > 1 {
            if let Some(addr) = self.retry_source_override_v6.take() {
                return Some(addr);
            }
        }
        self.source_override_v6
    }

    pub fn with_query_wire(mut self, wire: Vec<u8>) -> Self {
        self.query_wire = wire;
        self
    }

    pub fn with_listener_label(mut self, label: impl Into<String>) -> Self {
        self.listener_label = Some(label.into());
        self
    }

    pub fn with_global_query_index(mut self, global_query_index: u64) -> Self {
        self.global_query_index = global_query_index;
        self
    }

    pub fn qtype_label(&self) -> Option<String> {
        self.qtype.map(conduit_dns_wire::qtype_canonical_name)
    }

    pub fn rcode_label(&self) -> Option<String> {
        self.rcode.map(conduit_dns_wire::rcode_canonical_name)
    }

    pub fn set_rcode_name(&mut self, name: &str) {
        self.rcode = Some(
            conduit_dns_wire::Rcode::parse_name(name)
                .map(conduit_dns_wire::Rcode::number)
                .unwrap_or(2),
        );
    }

    pub fn set_rcode(&mut self, code: u16) {
        self.rcode = Some(code);
    }

    pub fn rcode(&self) -> Option<u16> {
        self.rcode
    }

    pub fn record_attempt(&mut self, pool: String, backend: SocketAddr, backend_label: String) {
        self.attempt_count += 1;
        self.attempts.push(AttemptRecord {
            pool: pool.clone(),
            backend,
            attempt: self.attempt_count,
        });
        self.selected_pool = Some(pool);
        self.selected_backend = Some(backend);
        self.selected_backend_label = Some(backend_label);
    }

    /// Logical label for the selected backend (configured `name` when set, else
    /// address). Falls back to the raw address for transactions that set
    /// `selected_backend` directly without a resolved label.
    pub fn selected_backend_display(&self) -> Option<String> {
        self.selected_backend_label
            .clone()
            .or_else(|| self.selected_backend.map(|a| a.to_string()))
    }

    pub fn take_retry_pool(&mut self) -> Option<String> {
        self.retry_pool.take()
    }

    pub fn set_soft_drop(&mut self) {
        self.soft_drop = true;
    }

    pub fn clear_soft_drop(&mut self) {
        self.soft_drop = false;
        self.dropped = false;
    }

    pub fn clear_retry_pool(&mut self) {
        self.retry_pool = None;
    }

    pub fn clear_pool(&mut self) {
        self.selected_pool = None;
    }
}

fn forward_elapsed_ms(started: Instant) -> u64 {
    u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use super::TagSet;

    #[test]
    fn tag_set_clear_removes_bool_flag() {
        let mut tags = TagSet::default();
        tags.set_bool("flag", true);
        assert!(tags.has("flag"));
        tags.clear("flag");
        assert!(!tags.has("flag"));
    }

    #[test]
    fn tag_set_clear_removes_string_tag() {
        let mut tags = TagSet::default();
        tags.set_string("tier", "vip");
        assert!(tags.has("tier"));
        tags.clear("tier");
        assert!(!tags.has("tier"));
    }

    #[test]
    fn tag_set_clear_removes_both_maps_for_key() {
        let mut tags = TagSet::default();
        tags.set_bool("mixed", true);
        tags.set_string("mixed", "value");
        tags.clear("mixed");
        assert!(!tags.has("mixed"));
        assert!(!tags.bool_flags().contains_key("mixed"));
    }

    #[test]
    fn take_effective_source_ignores_retry_stash_on_first_forward() {
        use super::Transaction;
        use crate::transaction::ClientProtocol;
        use std::net::SocketAddr;

        let mut txn = Transaction::new(
            1,
            "127.0.0.1:53".parse::<SocketAddr>().unwrap(),
            ClientProtocol::Udp,
        );
        txn.source_override_v4 = Some("127.0.0.1".parse().unwrap());
        txn.retry_source_override_v4 = Some("10.0.0.5".parse().unwrap());
        txn.attempt_count = 1;
        assert_eq!(
            txn.take_effective_source_override_v4(),
            Some("127.0.0.1".parse().unwrap())
        );
        assert_eq!(
            txn.retry_source_override_v4,
            Some("10.0.0.5".parse().unwrap())
        );
    }

    #[test]
    fn take_effective_source_consumes_retry_stash_on_retry_forward() {
        use super::Transaction;
        use crate::transaction::ClientProtocol;
        use std::net::SocketAddr;

        let mut txn = Transaction::new(
            1,
            "127.0.0.1:53".parse::<SocketAddr>().unwrap(),
            ClientProtocol::Udp,
        );
        txn.set_retry_source_override_v6("::1".parse().unwrap());
        txn.attempt_count = 2;
        assert_eq!(
            txn.take_effective_source_override_v6(),
            Some("::1".parse().unwrap())
        );
        assert!(txn.retry_source_override_v6.is_none());
    }

    #[test]
    fn complete_forward_rtt_records_milliseconds() {
        use super::Transaction;
        use crate::transaction::ClientProtocol;
        use std::net::SocketAddr;
        use std::thread;
        use std::time::{Duration, Instant};

        let mut txn = Transaction::new(
            1,
            "127.0.0.1:53".parse::<SocketAddr>().unwrap(),
            ClientProtocol::Udp,
        );
        assert_eq!(txn.last_forward_ms(), 0);

        let started = Instant::now();
        txn.mark_forward_started(started);
        thread::sleep(Duration::from_millis(5));
        txn.complete_forward_rtt(started);
        assert!(txn.last_forward_ms() >= 5);
        assert!(txn.forward_started_at.is_none());

        txn.mark_forward_started(Instant::now());
        thread::sleep(Duration::from_millis(3));
        txn.complete_forward_rtt_from_mark();
        assert!(txn.last_forward_ms() >= 3);
    }
}
