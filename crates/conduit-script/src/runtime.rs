use crate::compile::{CompiledScript, CompiledScripting, ScriptLimits};
use crate::data_sources::DataSourceStore;
use crate::dns_wire::{self, DnsOpcode, EdnsOptionCode, QueryClass, Rcode, RecordType};
use crate::host::{
    unix_secs, utc_hour_and_weekday, ClientProtocol, HostTransaction, ResponseWireMeta, ScriptPhase,
};
use crate::host_api::{register_host_surfaces, LogView, LookupView, MetricsView, RuntimeView};
use crate::metrics::MetricRegistry;
use crate::routing_view::RoutingRuntimeSnapshot;
use crate::script_errors::{report_lookup_unknown_table, report_script_eval_error};
use conduit_events::{hash_sample_keyed, matches_every_nth_global, matches_every_nth_worker};
use conduit_metrics::BuiltinRegistry;
use rhai::{Dynamic, Engine, EvalAltResult, Scope};
#[cfg(test)]
use std::cell::Cell;
use std::cell::RefCell;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime};

#[cfg(test)]
thread_local! {
    static ENGINE_BUILDS: Cell<u64> = const { Cell::new(0) };
}

thread_local! {
    pub(crate) static SCRIPT_RUN_CTX: RefCell<Option<ScriptRunContext>> = const { RefCell::new(None) };
    static LOOKUP_DATA: RefCell<Option<Arc<DataSourceStore>>> = const { RefCell::new(None) };
    static SCRIPT_RUNTIME: RefCell<Option<ScriptRuntime>> = const { RefCell::new(None) };
}

pub(crate) struct ScriptRunContext {
    pub(crate) script_path: String,
    pub(crate) rule_name: String,
    pub(crate) snapshot_generation: u64,
    pub(crate) txn_id: u64,
    pub(crate) builtin: Option<Arc<BuiltinRegistry>>,
}

struct RunOneResources {
    data: Arc<DataSourceStore>,
    metrics: Arc<MetricRegistry>,
    routing: Arc<RoutingRuntimeSnapshot>,
    snapshot_generation: u64,
    builtin: Option<Arc<BuiltinRegistry>>,
}

struct ScriptRuntime {
    engine: Engine,
    snapshot_generation: u64,
}

impl ScriptRuntime {
    fn new(scripting: &CompiledScripting) -> Self {
        #[cfg(test)]
        ENGINE_BUILDS.with(|count| count.set(count.get() + 1));

        let mut engine = Engine::new();
        register_host_api(&mut engine);

        Self {
            engine,
            snapshot_generation: scripting.snapshot_generation,
        }
    }
}

fn register_host_api(engine: &mut Engine) {
    engine.register_fn("lookup", |table: &str, key: &str| -> String {
        LOOKUP_DATA.with(|cell| {
            let store = cell.borrow().clone();
            let Some(store) = store else {
                return String::new();
            };
            if !store.has_table(table) {
                SCRIPT_RUN_CTX.with(|ctx_cell| {
                    if let Some(ctx) = ctx_cell.borrow().as_ref() {
                        report_lookup_unknown_table(
                            ctx.builtin.as_deref(),
                            ctx.snapshot_generation,
                            &ctx.script_path,
                            &ctx.rule_name,
                            table,
                        );
                    }
                });
                return String::new();
            }
            store.lookup(table, key)
        })
    });

    register_host_surfaces(engine);

    engine
        .register_type_with_name::<RhaiTxn>("Transaction")
        .register_fn("question", RhaiTxn::question)
        .register_fn("response", RhaiTxn::response)
        .register_fn("response_rcode", RhaiTxn::response_rcode)
        .register_fn("set_tag", RhaiTxn::set_tag)
        .register_fn("has_tag", RhaiTxn::has_tag)
        .register_fn("clear_tag", RhaiTxn::clear_tag)
        .register_fn("set_pool", RhaiTxn::set_pool)
        .register_fn("set_retry_pool", RhaiTxn::set_retry_pool)
        .register_fn("request_retry", RhaiTxn::request_retry)
        .register_fn("request_retry_now", RhaiTxn::request_retry_now)
        .register_fn("drop_query", RhaiTxn::drop_query)
        .register_fn("drop_query_now", RhaiTxn::drop_query_now)
        .register_fn("clear_drop", RhaiTxn::clear_drop)
        .register_fn("clear_retry", RhaiTxn::clear_retry)
        .register_fn("clear_retry_pool", RhaiTxn::clear_retry_pool)
        .register_fn("clear_pool", RhaiTxn::clear_pool)
        .register_fn("set_rcode", RhaiTxn::set_rcode_enum)
        .register_fn("set_rcode", RhaiTxn::set_rcode_name)
        .register_fn("set_source_v4", RhaiTxn::set_source_v4)
        .register_fn("set_source_v6", RhaiTxn::set_source_v6)
        .register_fn("set_retry_source_v4", RhaiTxn::set_retry_source_v4)
        .register_fn("set_retry_source_v6", RhaiTxn::set_retry_source_v6)
        .register_fn("clear_retry_source_v4", RhaiTxn::clear_retry_source_v4)
        .register_fn("clear_retry_source_v6", RhaiTxn::clear_retry_source_v6)
        .register_fn("sample_percent", RhaiTxn::sample_percent)
        .register_fn("sample_percent", RhaiTxn::sample_percent_keyed)
        .register_fn(
            "sample_percent_for_qname",
            RhaiTxn::sample_percent_for_qname,
        )
        .register_fn("sample_percent_for_rule", RhaiTxn::sample_percent_for_rule)
        .register_fn("every_nth_worker", RhaiTxn::every_nth_worker)
        .register_fn("every_nth_global", RhaiTxn::every_nth_global)
        .register_fn("rule_name", RhaiTxn::rule_name)
        .register_fn("txn_id", RhaiTxn::txn_id)
        .register_fn("config_generation", RhaiTxn::config_generation)
        .register_fn("client_addr", RhaiTxn::client_addr)
        .register_fn("client_ip", RhaiTxn::client_ip)
        .register_fn("client_port", RhaiTxn::client_port)
        .register_fn("client_protocol", RhaiTxn::client_protocol)
        .register_fn("listener", RhaiTxn::listener)
        .register_fn("now_unix", RhaiTxn::now_unix)
        .register_fn("utc_hour", RhaiTxn::utc_hour)
        .register_fn("utc_weekday", RhaiTxn::utc_weekday)
        .register_fn("selected_pool", RhaiTxn::selected_pool)
        .register_fn("selected_backend", RhaiTxn::selected_backend)
        .register_fn("selected_backend_name", RhaiTxn::selected_backend_name)
        .register_fn("response_truncated", RhaiTxn::response_truncated)
        .register_fn("response_answer_count", RhaiTxn::response_answer_count)
        .register_fn(
            "response_authority_count",
            RhaiTxn::response_authority_count,
        )
        .register_fn(
            "response_additional_count",
            RhaiTxn::response_additional_count,
        )
        .register_fn("response_authoritative", RhaiTxn::response_authoritative)
        .register_fn("elapsed_ms", RhaiTxn::elapsed_ms)
        .register_fn("get_attempt_count", RhaiTxn::attempt_count)
        .register_fn("last_forward_ms", RhaiTxn::last_forward_ms)
        .register_fn(
            "set_cache_lookup_eligible",
            RhaiTxn::set_cache_lookup_eligible,
        )
        .register_fn("answer_source", RhaiTxn::answer_source)
        .register_fn("cache_instance", RhaiTxn::cache_instance);

    dns_wire::register_dns_wire_api(engine);
}

fn with_runtime<F, R>(scripting: &CompiledScripting, f: F) -> R
where
    F: FnOnce(&mut ScriptRuntime) -> R,
{
    SCRIPT_RUNTIME.with(|cell| {
        let mut slot = cell.borrow_mut();
        if slot
            .as_ref()
            .map(|rt| rt.snapshot_generation != scripting.snapshot_generation)
            .unwrap_or(true)
        {
            *slot = Some(ScriptRuntime::new(scripting));
        }
        f(slot.as_mut().expect("script runtime initialized"))
    })
}

#[cfg(test)]
pub(crate) fn reset_thread_runtime_for_tests() {
    SCRIPT_RUNTIME.with(|cell| *cell.borrow_mut() = None);
    LOOKUP_DATA.with(|cell| *cell.borrow_mut() = None);
    #[cfg(test)]
    ENGINE_BUILDS.with(|count| count.set(0));
}

#[cfg(test)]
pub(crate) fn thread_runtime_engine_builds() -> u64 {
    ENGINE_BUILDS.with(|count| count.get())
}

#[derive(Debug, Default, Clone)]
pub struct UserMetricFlush {
    pub name: String,
    pub labels: HashMap<String, String>,
    pub delta: u64,
}

#[derive(Debug, Default, Clone)]
pub struct ScriptRunStats {
    pub errors: u32,
    pub user_metrics: Vec<UserMetricFlush>,
    /// Soft-retry intent was cleared in script (`txn.clear_retry()`).
    pub clear_soft_retry: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScriptRunOutcome {
    Ok,
    Error,
    /// Soft drop — caller may continue other actions on the rule.
    Drop,
    /// Hard drop — stop further actions on the rule.
    DropNow,
    /// Soft retry — caller may continue other actions on the rule.
    Retry,
    /// Hard retry — stop further actions on the rule (soft drop still wins).
    RetryNow,
}

#[derive(Debug, Default, Clone)]
pub(crate) struct ScriptEffects {
    pool: Option<String>,
    retry_pool: Option<String>,
    retry_requested: bool,
    hard_retry: bool,
    clear_soft_retry: bool,
    clear_retry_pool: bool,
    clear_pool: bool,
    tag_ops: HashMap<String, TagOp>,
    rcode: Option<u16>,
    soft_drop: bool,
    hard_drop: bool,
    clear_soft_drop: bool,
    sample_decisions: HashMap<(u16, String), bool>,
    source_override_v4: Option<std::net::Ipv4Addr>,
    source_override_v6: Option<std::net::Ipv6Addr>,
    retry_source_override_v4: Option<std::net::Ipv4Addr>,
    retry_source_override_v6: Option<std::net::Ipv6Addr>,
    clear_retry_source_v4: bool,
    clear_retry_source_v6: bool,
    cache_lookup_eligible: Option<bool>,
    user_metric_flushes: Vec<UserMetricFlush>,
}

#[derive(Debug, Clone)]
enum TagOp {
    Bool(bool),
    String(String),
    Clear,
}

#[derive(Clone)]
pub(crate) struct RhaiTxn {
    phase: ScriptPhase,
    txn_id: u64,
    global_query_index: u64,
    config_generation: u64,
    rule_name: String,
    qname: Option<String>,
    qtype: Option<u16>,
    qclass: Option<u16>,
    opcode: Option<u8>,
    edns_option_codes: Vec<u16>,
    dns_id: u16,
    rcode: Option<u16>,
    client_addr: SocketAddr,
    client_protocol: ClientProtocol,
    listener_label: Option<String>,
    received_at: SystemTime,
    selected_pool: Option<String>,
    selected_backend: Option<SocketAddr>,
    selected_backend_label: Option<String>,
    response_meta: Option<ResponseWireMeta>,
    attempt_count: u32,
    started_at: Instant,
    last_forward_ms: u64,
    answer_source: Option<String>,
    cache_instance: Option<String>,
    tags_snapshot_bools: HashMap<String, bool>,
    tags_snapshot_strings: HashMap<String, String>,
    effects: Arc<Mutex<ScriptEffects>>,
}

impl RhaiTxn {
    fn insert_question_fields(&self, map: &mut rhai::Map) {
        if let Some(ref qname) = self.qname {
            map.insert("qname".into(), Dynamic::from(qname.clone()));
        }
        if let Some(qtype) = self.qtype {
            map.insert("qtype".into(), Dynamic::from(RecordType::from(qtype)));
        }
        if let Some(qclass) = self.qclass {
            map.insert("qclass".into(), Dynamic::from(QueryClass::from(qclass)));
        }
        if let Some(opcode) = self.opcode {
            map.insert(
                "opcode".into(),
                Dynamic::from(DnsOpcode(conduit_dns_wire::DnsOpcode(opcode))),
            );
        }
        if !self.edns_option_codes.is_empty() {
            let options: rhai::Array = self
                .edns_option_codes
                .iter()
                .map(|&code| Dynamic::from(EdnsOptionCode::from(code)))
                .collect();
            map.insert("edns_options".into(), Dynamic::from(options));
        }
        map.insert("id".into(), Dynamic::from(self.dns_id as i64));
    }

    fn question(&mut self) -> Dynamic {
        let mut map = rhai::Map::new();
        self.insert_question_fields(&mut map);
        Dynamic::from(map)
    }

    fn response(&mut self) -> Result<Dynamic, Box<EvalAltResult>> {
        if self.phase != ScriptPhase::Response {
            return Err("response() is not available in request phase".into());
        }
        let mut map = rhai::Map::new();
        if let Some(rcode) = self.rcode {
            map.insert("rcode".into(), Dynamic::from(Rcode::from(rcode)));
        }
        self.insert_question_fields(&mut map);
        self.insert_response_path_fields(&mut map);
        Ok(Dynamic::from(map))
    }

    fn insert_response_path_fields(&self, map: &mut rhai::Map) {
        if let Some(ref pool) = self.selected_pool {
            map.insert("pool".into(), Dynamic::from(pool.clone()));
        }
        if let Some(backend) = self.selected_backend {
            map.insert("backend".into(), Dynamic::from(backend.to_string()));
        }
        if let Some(label) = self
            .selected_backend_label
            .clone()
            .or_else(|| self.selected_backend.map(|a| a.to_string()))
        {
            map.insert("backend_name".into(), Dynamic::from(label));
        }
        if let Some(ref src) = self.answer_source {
            map.insert("answer_source".into(), Dynamic::from(src.clone()));
        }
        if let Some(ref cache) = self.cache_instance {
            map.insert("cache_instance".into(), Dynamic::from(cache.clone()));
        }
        if let Some(meta) = self.response_meta {
            map.insert(
                "answer_count".into(),
                Dynamic::from(meta.answer_count as i64),
            );
            map.insert(
                "authority_count".into(),
                Dynamic::from(meta.authority_count as i64),
            );
            map.insert(
                "additional_count".into(),
                Dynamic::from(meta.additional_count as i64),
            );
            map.insert("truncated".into(), Dynamic::from(meta.truncated));
            map.insert("authoritative".into(), Dynamic::from(meta.authoritative));
        }
    }

    fn response_rcode(&mut self) -> Dynamic {
        if self.phase != ScriptPhase::Response {
            return Dynamic::UNIT;
        }
        self.rcode
            .map(|rcode| Dynamic::from(Rcode::from(rcode)))
            .unwrap_or(Dynamic::UNIT)
    }

    fn set_tag(&mut self, key: &str, value: Dynamic) -> Result<(), Box<EvalAltResult>> {
        let mut fx = self.effects.lock().map_err(|e| e.to_string())?;
        if value.is::<bool>() {
            fx.tag_ops.insert(
                key.to_string(),
                TagOp::Bool(value.as_bool().unwrap_or(false)),
            );
        } else {
            fx.tag_ops
                .insert(key.to_string(), TagOp::String(value.to_string()));
        }
        Ok(())
    }

    fn clear_tag(&mut self, key: &str) {
        if let Ok(mut fx) = self.effects.lock() {
            fx.tag_ops.insert(key.to_string(), TagOp::Clear);
        }
    }

    fn has_tag(&mut self, key: &str) -> bool {
        if let Ok(fx) = self.effects.lock() {
            if let Some(op) = fx.tag_ops.get(key) {
                return match op {
                    TagOp::Clear => false,
                    TagOp::Bool(v) => *v,
                    TagOp::String(_) => true,
                };
            }
        }
        if self.tags_snapshot_bools.get(key).copied().unwrap_or(false) {
            return true;
        }
        self.tags_snapshot_strings.contains_key(key)
    }

    fn set_pool(&mut self, name: &str) {
        if let Ok(mut fx) = self.effects.lock() {
            fx.pool = Some(name.to_string());
            fx.clear_pool = false;
        }
    }

    fn set_retry_pool(&mut self, pool: &str) {
        if let Ok(mut fx) = self.effects.lock() {
            fx.retry_pool = Some(pool.to_string());
        }
    }

    fn request_retry(&mut self) {
        if self.phase != ScriptPhase::Response {
            return;
        }
        if let Ok(mut fx) = self.effects.lock() {
            fx.retry_requested = true;
        }
    }

    fn request_retry_now(&mut self) {
        if self.phase != ScriptPhase::Response {
            return;
        }
        if let Ok(mut fx) = self.effects.lock() {
            fx.hard_retry = true;
        }
    }

    fn drop_query(&mut self) {
        if let Ok(mut fx) = self.effects.lock() {
            fx.soft_drop = true;
        }
    }

    fn drop_query_now(&mut self) {
        if let Ok(mut fx) = self.effects.lock() {
            fx.hard_drop = true;
        }
    }

    fn clear_drop(&mut self) {
        if let Ok(mut fx) = self.effects.lock() {
            fx.clear_soft_drop = true;
        }
    }

    fn clear_retry(&mut self) {
        if self.phase != ScriptPhase::Response {
            return;
        }
        if let Ok(mut fx) = self.effects.lock() {
            fx.clear_soft_retry = true;
        }
    }

    fn clear_retry_pool(&mut self) {
        if let Ok(mut fx) = self.effects.lock() {
            fx.clear_retry_pool = true;
        }
    }

    fn clear_pool(&mut self) {
        if let Ok(mut fx) = self.effects.lock() {
            fx.pool = None;
            fx.clear_pool = true;
        }
    }

    fn set_rcode_enum(&mut self, rcode: Rcode) {
        if let Ok(mut fx) = self.effects.lock() {
            fx.rcode = Some(rcode.number());
        }
    }

    fn set_rcode_name(&mut self, name: &str) {
        if let Ok(mut fx) = self.effects.lock() {
            fx.rcode = Rcode::parse_name(name).map(Rcode::number);
        }
    }

    fn set_source_v4(&mut self, addr: &str) -> Result<(), Box<EvalAltResult>> {
        if self.phase != ScriptPhase::Request {
            return Err("set_source_v4() is not available in response phase".into());
        }
        let ip: std::net::Ipv4Addr = addr
            .parse()
            .map_err(|_| format!("set_source_v4: '{addr}' is not a valid IPv4 address"))?;
        let mut fx = self.effects.lock().map_err(|e| e.to_string())?;
        fx.source_override_v4 = Some(ip);
        Ok(())
    }

    fn set_source_v6(&mut self, addr: &str) -> Result<(), Box<EvalAltResult>> {
        if self.phase != ScriptPhase::Request {
            return Err("set_source_v6() is not available in response phase".into());
        }
        let ip: std::net::Ipv6Addr = addr
            .parse()
            .map_err(|_| format!("set_source_v6: '{addr}' is not a valid IPv6 address"))?;
        let mut fx = self.effects.lock().map_err(|e| e.to_string())?;
        fx.source_override_v6 = Some(ip);
        Ok(())
    }

    fn set_retry_source_v4(&mut self, addr: &str) -> Result<(), Box<EvalAltResult>> {
        let ip: std::net::Ipv4Addr = addr
            .parse()
            .map_err(|_| format!("set_retry_source_v4: '{addr}' is not a valid IPv4 address"))?;
        let mut fx = self.effects.lock().map_err(|e| e.to_string())?;
        fx.retry_source_override_v4 = Some(ip);
        Ok(())
    }

    fn set_retry_source_v6(&mut self, addr: &str) -> Result<(), Box<EvalAltResult>> {
        let ip: std::net::Ipv6Addr = addr
            .parse()
            .map_err(|_| format!("set_retry_source_v6: '{addr}' is not a valid IPv6 address"))?;
        let mut fx = self.effects.lock().map_err(|e| e.to_string())?;
        fx.retry_source_override_v6 = Some(ip);
        Ok(())
    }

    fn clear_retry_source_v4(&mut self) {
        if let Ok(mut fx) = self.effects.lock() {
            fx.clear_retry_source_v4 = true;
        }
    }

    fn clear_retry_source_v6(&mut self) {
        if let Ok(mut fx) = self.effects.lock() {
            fx.clear_retry_source_v6 = true;
        }
    }

    fn sample_percent(&mut self, percent: f64) -> bool {
        self.sample_percent_keyed(percent, "")
    }

    fn sample_percent_keyed(&mut self, percent: f64, key: &str) -> bool {
        let Ok(mut fx) = self.effects.lock() else {
            return false;
        };
        let clamped = percent.clamp(0.0, 100.0);
        let percent_key = (clamped * 100.0).round() as u16;
        let cache_key = (percent_key, key.to_string());
        if let Some(decision) = fx.sample_decisions.get(&cache_key) {
            return *decision;
        }
        let salt = if key.is_empty() { None } else { Some(key) };
        let decision = hash_sample_keyed(self.txn_id, clamped / 100.0, salt);
        fx.sample_decisions.insert(cache_key, decision);
        if decision {
            fx.tag_ops.insert("sampled".into(), TagOp::Bool(true));
        }
        decision
    }

    fn sample_percent_for_qname(&mut self, percent: f64) -> bool {
        let Some(qname) = self.qname.clone() else {
            return false;
        };
        self.sample_percent_keyed(percent, &qname)
    }

    fn sample_percent_for_rule(&mut self, percent: f64) -> bool {
        let rule_name = self.rule_name.clone();
        self.sample_percent_keyed(percent, &rule_name)
    }

    fn every_nth_worker(&mut self, nth: i64) -> Result<bool, Box<EvalAltResult>> {
        let nth = parse_every_nth_arg(nth)?;
        Ok(matches_every_nth_worker(self.txn_id, nth))
    }

    fn every_nth_global(&mut self, nth: i64) -> Result<bool, Box<EvalAltResult>> {
        let nth = parse_every_nth_arg(nth)?;
        Ok(matches_every_nth_global(self.global_query_index, nth))
    }

    fn rule_name(&mut self) -> String {
        self.rule_name.clone()
    }

    fn txn_id(&mut self) -> i64 {
        i64::try_from(self.txn_id).unwrap_or(i64::MAX)
    }

    fn config_generation(&mut self) -> i64 {
        i64::try_from(self.config_generation).unwrap_or(i64::MAX)
    }

    fn client_addr(&mut self) -> String {
        self.client_addr.to_string()
    }

    fn client_ip(&mut self) -> String {
        self.client_addr.ip().to_string()
    }

    fn client_port(&mut self) -> i64 {
        self.client_addr.port() as i64
    }

    fn client_protocol(&mut self) -> String {
        match self.client_protocol {
            ClientProtocol::Udp => "udp".into(),
            ClientProtocol::Tcp => "tcp".into(),
        }
    }

    fn listener(&mut self) -> String {
        self.listener_label.clone().unwrap_or_default()
    }

    fn now_unix(&mut self) -> i64 {
        i64::try_from(unix_secs(self.received_at)).unwrap_or(0)
    }

    fn utc_hour(&mut self) -> i64 {
        let (hour, _) = utc_hour_and_weekday(unix_secs(self.received_at));
        hour as i64
    }

    fn utc_weekday(&mut self) -> i64 {
        let (_, weekday) = utc_hour_and_weekday(unix_secs(self.received_at));
        weekday as i64
    }

    pub(crate) fn selected_pool(&mut self) -> String {
        self.selected_pool.clone().unwrap_or_default()
    }

    fn selected_backend(&mut self) -> String {
        self.selected_backend
            .map(|a| a.to_string())
            .unwrap_or_default()
    }

    pub(crate) fn selected_backend_name(&mut self) -> String {
        self.selected_backend_label
            .clone()
            .or_else(|| self.selected_backend.map(|a| a.to_string()))
            .unwrap_or_default()
    }

    fn response_truncated(&mut self) -> bool {
        self.response_meta.map(|m| m.truncated).unwrap_or(false)
    }

    fn response_answer_count(&mut self) -> i64 {
        self.response_meta
            .map(|m| m.answer_count as i64)
            .unwrap_or(-1)
    }

    fn response_authority_count(&mut self) -> i64 {
        self.response_meta
            .map(|m| m.authority_count as i64)
            .unwrap_or(-1)
    }

    fn response_additional_count(&mut self) -> i64 {
        self.response_meta
            .map(|m| m.additional_count as i64)
            .unwrap_or(-1)
    }

    fn response_authoritative(&mut self) -> bool {
        self.response_meta.map(|m| m.authoritative).unwrap_or(false)
    }

    fn elapsed_ms(&mut self) -> i64 {
        self.started_at.elapsed().as_millis() as i64
    }

    fn last_forward_ms(&mut self) -> i64 {
        self.last_forward_ms as i64
    }

    fn set_cache_lookup_eligible(&mut self, eligible: bool) {
        if self.phase != ScriptPhase::Request {
            return;
        }
        if let Ok(mut fx) = self.effects.lock() {
            fx.cache_lookup_eligible = Some(eligible);
        }
    }

    fn answer_source(&mut self) -> String {
        if self.phase != ScriptPhase::Response {
            return String::new();
        }
        self.answer_source.clone().unwrap_or_default()
    }

    fn cache_instance(&mut self) -> String {
        if self.phase != ScriptPhase::Response {
            return String::new();
        }
        self.cache_instance.clone().unwrap_or_default()
    }

    fn attempt_count(&mut self) -> i64 {
        self.attempt_count as i64
    }
}

pub(crate) fn queue_user_metric(
    registry: &MetricRegistry,
    effects: &Arc<Mutex<ScriptEffects>>,
    name: &str,
    delta: i64,
    label_pairs: &[(String, String)],
) -> Result<(), Box<EvalAltResult>> {
    let label_map: HashMap<String, String> = label_pairs.iter().cloned().collect();
    registry
        .validate_runtime_labels(name, &label_map)
        .map_err(|e| e.to_string())?;
    let mut fx = effects.lock().map_err(|e| e.to_string())?;
    fx.user_metric_flushes.push(UserMetricFlush {
        name: name.to_string(),
        labels: label_map,
        delta: delta.max(0) as u64,
    });
    Ok(())
}

fn parse_every_nth_arg(nth: i64) -> Result<u64, Box<EvalAltResult>> {
    if nth < 1 {
        return Err("every_nth value must be >= 1".into());
    }
    u64::try_from(nth).map_err(|_| "every_nth value out of range".into())
}

#[allow(clippy::too_many_arguments)]
pub fn run_scripts(
    scripting: &CompiledScripting,
    script_ids: &[usize],
    host: &mut dyn HostTransaction,
    phase: ScriptPhase,
    user_export: Option<&conduit_metrics::UserRegistry>,
    builtin_profile: Option<conduit_metrics::BuiltinProfile>,
    builtin: Option<Arc<BuiltinRegistry>>,
    routing_runtime: Option<Arc<RoutingRuntimeSnapshot>>,
) -> (ScriptRunOutcome, ScriptRunStats) {
    if script_ids.is_empty() {
        return (ScriptRunOutcome::Ok, ScriptRunStats::default());
    }

    let mut stats = ScriptRunStats::default();
    let data = Arc::clone(&scripting.data_sources);
    let metrics = Arc::new(scripting.metrics.clone());
    let routing = routing_runtime.unwrap_or_else(|| Arc::new(RoutingRuntimeSnapshot::default()));

    for &id in script_ids {
        let Some(script) = scripting.scripts.get(id) else {
            continue;
        };
        if script.hook != phase {
            continue;
        }
        let run_result = with_runtime(scripting, |runtime| {
            run_one(
                runtime,
                script,
                &scripting.limits,
                host,
                phase,
                RunOneResources {
                    data: data.clone(),
                    metrics: metrics.clone(),
                    routing: routing.clone(),
                    snapshot_generation: scripting.snapshot_generation,
                    builtin: builtin.clone(),
                },
            )
        });
        match run_result {
            Ok(fx) => {
                apply_effects(host, &fx);
                stats.user_metrics.extend(fx.user_metric_flushes.clone());
                if let Some(export) = user_export {
                    let profile = builtin_profile.unwrap_or(conduit_metrics::BuiltinProfile::Off);
                    for m in &fx.user_metric_flushes {
                        if scripting.metrics.exports_at_profile(&m.name, profile) {
                            export.add_delta(conduit_metrics::UserMetricDelta {
                                name: m.name.clone(),
                                labels: m.labels.clone(),
                                delta: m.delta,
                            });
                        }
                    }
                }
                if fx.hard_drop {
                    return (ScriptRunOutcome::DropNow, stats);
                }
                if fx.soft_drop {
                    return (ScriptRunOutcome::Drop, stats);
                }
                if fx.hard_retry {
                    return (ScriptRunOutcome::RetryNow, stats);
                }
                if fx.clear_soft_retry {
                    stats.clear_soft_retry = true;
                }
                if fx.retry_requested && !fx.clear_soft_retry {
                    return (ScriptRunOutcome::Retry, stats);
                }
            }
            Err(e) => {
                stats.errors += 1;
                report_script_eval_error(builtin.as_deref(), &script.path, &script.rule_name, &e);
            }
        }
    }

    (ScriptRunOutcome::Ok, stats)
}

fn apply_effects(host: &mut dyn HostTransaction, fx: &ScriptEffects) {
    if fx.clear_pool {
        host.clear_pool();
    } else if let Some(ref pool) = fx.pool {
        host.set_pool(pool);
    }
    if let Some(ref pool) = fx.retry_pool {
        host.set_retry_pool(pool);
    }
    let mut tag_keys: Vec<_> = fx.tag_ops.keys().cloned().collect();
    tag_keys.sort();
    for key in tag_keys {
        match fx.tag_ops.get(&key).expect("key from sorted keys") {
            TagOp::Bool(v) => host.set_tag_bool(&key, *v),
            TagOp::String(v) => host.set_tag_string(&key, v),
            TagOp::Clear => host.clear_tag(&key),
        }
    }
    if let Some(rc) = fx.rcode {
        host.set_rcode_number(rc);
    }
    if let Some(addr) = fx.source_override_v4 {
        host.set_source_v4(&addr.to_string());
    }
    if let Some(addr) = fx.source_override_v6 {
        host.set_source_v6(&addr.to_string());
    }
    if let Some(addr) = fx.retry_source_override_v4 {
        host.set_retry_source_v4(&addr.to_string());
    }
    if let Some(addr) = fx.retry_source_override_v6 {
        host.set_retry_source_v6(&addr.to_string());
    }
    if fx.clear_retry_source_v4 {
        host.clear_retry_source_v4();
    }
    if fx.clear_retry_source_v6 {
        host.clear_retry_source_v6();
    }
    if fx.clear_soft_drop {
        host.clear_soft_drop();
    }
    if fx.clear_retry_pool {
        host.clear_retry_pool();
    }
    if fx.soft_drop {
        host.set_soft_drop();
    }
    if fx.hard_drop {
        host.mark_dropped();
    }
    if let Some(eligible) = fx.cache_lookup_eligible {
        host.set_cache_lookup_eligible(eligible);
    }
}

fn run_one(
    runtime: &mut ScriptRuntime,
    script: &CompiledScript,
    limits: &ScriptLimits,
    host: &dyn HostTransaction,
    phase: ScriptPhase,
    resources: RunOneResources,
) -> Result<ScriptEffects, String> {
    let RunOneResources {
        data,
        metrics,
        routing,
        snapshot_generation,
        builtin,
    } = resources;
    let effects = Arc::new(Mutex::new(ScriptEffects::default()));
    let txn = RhaiTxn {
        phase,
        txn_id: host.txn_id(),
        global_query_index: host.global_query_index(),
        config_generation: host.snapshot_generation(),
        rule_name: script.rule_name.clone(),
        qname: host.question_qname().map(str::to_string),
        qtype: host.question_qtype(),
        qclass: host.question_qclass(),
        opcode: host.question_opcode(),
        edns_option_codes: host.question_edns_option_codes().to_vec(),
        dns_id: host.question_id(),
        rcode: host.response_rcode_number(),
        client_addr: host.client_addr(),
        client_protocol: host.client_protocol(),
        listener_label: host.listener_label().map(str::to_string),
        received_at: host.received_at(),
        selected_pool: host.selected_pool().map(str::to_string),
        selected_backend: host.selected_backend(),
        selected_backend_label: host.selected_backend_label(),
        response_meta: host.response_meta(),
        attempt_count: host.attempt_count(),
        started_at: host.started_at(),
        last_forward_ms: host.last_forward_ms(),
        answer_source: host.answer_source().map(str::to_string),
        cache_instance: host.cache_instance().map(str::to_string),
        tags_snapshot_bools: host.script_tag_bools(),
        tags_snapshot_strings: host.script_tag_strings(),
        effects: effects.clone(),
    };

    LOOKUP_DATA.with(|cell| *cell.borrow_mut() = Some(data.clone()));
    SCRIPT_RUN_CTX.with(|cell| {
        *cell.borrow_mut() = Some(ScriptRunContext {
            script_path: script.path.clone(),
            rule_name: script.rule_name.clone(),
            snapshot_generation,
            txn_id: host.txn_id(),
            builtin,
        });
    });

    let engine = &mut runtime.engine;
    engine.set_max_operations(limits.max_operations);
    engine.set_max_call_levels(limits.max_call_depth as usize);

    let timeout = Duration::from_millis(limits.hook_timeout_ms as u64);
    let start = Instant::now();
    engine.on_progress(move |_ops| {
        if start.elapsed() > timeout {
            Some("script hook timeout".into())
        } else {
            None
        }
    });

    let mut scope = Scope::new();
    scope.push("txn", txn);
    scope.push("runtime", RuntimeView::new(routing));
    scope.push("lookup", LookupView::new(data.clone()));
    scope.push(
        "metrics",
        MetricsView::new(metrics.clone(), effects.clone()),
    );
    scope.push("log", LogView);

    let result = engine
        .run_ast_with_scope(&mut scope, &script.ast)
        .map_err(|e| e.to_string());

    LOOKUP_DATA.with(|cell| *cell.borrow_mut() = None);
    SCRIPT_RUN_CTX.with(|cell| *cell.borrow_mut() = None);

    result?;

    let fx = effects.lock().map_err(|e| e.to_string())?;
    Ok(ScriptEffects {
        pool: fx.pool.clone(),
        retry_pool: fx.retry_pool.clone(),
        retry_requested: fx.retry_requested,
        hard_retry: fx.hard_retry,
        clear_soft_retry: fx.clear_soft_retry,
        clear_retry_pool: fx.clear_retry_pool,
        clear_pool: fx.clear_pool,
        tag_ops: fx.tag_ops.clone(),
        rcode: fx.rcode,
        soft_drop: fx.soft_drop,
        hard_drop: fx.hard_drop,
        clear_soft_drop: fx.clear_soft_drop,
        sample_decisions: HashMap::new(),
        source_override_v4: fx.source_override_v4,
        source_override_v6: fx.source_override_v6,
        retry_source_override_v4: fx.retry_source_override_v4,
        retry_source_override_v6: fx.retry_source_override_v6,
        clear_retry_source_v4: fx.clear_retry_source_v4,
        clear_retry_source_v6: fx.clear_retry_source_v6,
        cache_lookup_eligible: fx.cache_lookup_eligible,
        user_metric_flushes: fx.user_metric_flushes.clone(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compile::compile_from_config;
    use crate::host::ResponseWireMeta;
    use crate::rhai_script_errors_total;
    use crate::routing_view::PoolRoutingView;
    use crate::testing::MockHost;
    use conduit_config::load_yaml;
    use conduit_metrics::{BuiltinProfile, BuiltinRegistry};
    use prometheus::{Encoder, TextEncoder};
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::Arc;
    use std::time::UNIX_EPOCH;

    #[test]
    fn set_source_v4_via_script() {
        let script = r#"txn.set_source_v4("127.0.0.1");"#;
        let dir = std::env::temp_dir().join(format!("conduit-script-src-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let script_path = dir.join("src.rhai");
        std::fs::write(&script_path, script).unwrap();
        let yaml = format!(
            r#"
schema_version: 1
listeners:
  threads: 1
  reuse_port: false
  listeners:
    - address: "127.0.0.1:15353"
      protocol: udp
forward:
  outstanding_per_backend: 100
  timeout_ms: 2000
orchestrator:
  max_attempts: 3
  max_txn_duration_ms: 5000
  txn_table_capacity: 1024
events:
  queue_depth: 4096
  drop_policy: drop_oldest
rhai:
  max_operations: 10000
  max_call_depth: 32
pools:
  - name: default
    backends:
      - address: "127.0.0.1:5300"
control:
  listen_address: "127.0.0.1:5199"
rules:
  match_mode: first_match
  rules:
    - name: src
      hook: request
      selectors: []
      actions:
        - type: rhai
          value: "{}"
"#,
            script_path.display()
        );
        let cfg = load_yaml(&yaml).unwrap();
        let scripting = compile_from_config(&cfg, Some(&dir)).unwrap();
        let mut host = MockHost {
            id: 51,
            global_query_index: 0,
            qname: "test.example.".into(),
            qtype: 1,
            qclass: 1,
            opcode: 0,
            edns_option_codes: vec![],
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
            ..Default::default()
        };
        let (_, stats) = run_scripts(
            &scripting,
            &[0],
            &mut host,
            ScriptPhase::Request,
            None,
            None,
            None,
            None,
        );
        assert_eq!(stats.errors, 0);
        assert_eq!(host.source_override_v4, Some(std::net::Ipv4Addr::LOCALHOST));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn set_source_v6_via_script() {
        let script = r#"txn.set_source_v6("::1");"#;
        let dir = std::env::temp_dir().join(format!("conduit-script-src6-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let script_path = dir.join("src6.rhai");
        std::fs::write(&script_path, script).unwrap();
        let yaml = format!(
            r#"
schema_version: 1
listeners:
  threads: 1
  reuse_port: false
  listeners:
    - address: "127.0.0.1:15353"
      protocol: udp
forward:
  outstanding_per_backend: 100
  timeout_ms: 2000
orchestrator:
  max_attempts: 3
  max_txn_duration_ms: 5000
  txn_table_capacity: 1024
events:
  queue_depth: 4096
  drop_policy: drop_oldest
rhai:
  max_operations: 10000
  max_call_depth: 32
pools:
  - name: default
    backends:
      - address: "127.0.0.1:5300"
control:
  listen_address: "127.0.0.1:5199"
rules:
  match_mode: first_match
  rules:
    - name: src6
      hook: request
      selectors: []
      actions:
        - type: rhai
          value: "{}"
"#,
            script_path.display()
        );
        let cfg = load_yaml(&yaml).unwrap();
        let scripting = compile_from_config(&cfg, Some(&dir)).unwrap();
        let mut host = MockHost {
            id: 52,
            global_query_index: 0,
            qname: "test.example.".into(),
            qtype: 1,
            qclass: 1,
            opcode: 0,
            edns_option_codes: vec![],
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
            ..Default::default()
        };
        let (_, stats) = run_scripts(
            &scripting,
            &[0],
            &mut host,
            ScriptPhase::Request,
            None,
            None,
            None,
            None,
        );
        assert_eq!(stats.errors, 0);
        assert_eq!(host.source_override_v6, Some(std::net::Ipv6Addr::LOCALHOST));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn set_retry_source_v4_via_script_on_response() {
        let script = r#"txn.set_retry_source_v4("10.0.0.5");"#;
        let dir = std::env::temp_dir().join(format!("conduit-script-rsrc-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let script_path = dir.join("rsrc.rhai");
        std::fs::write(&script_path, script).unwrap();
        let yaml = format!(
            r#"
schema_version: 1
listeners:
  threads: 1
  reuse_port: false
  listeners:
    - address: "127.0.0.1:15353"
      protocol: udp
forward:
  outstanding_per_backend: 100
  timeout_ms: 2000
orchestrator:
  max_attempts: 3
  max_txn_duration_ms: 5000
  txn_table_capacity: 1024
events:
  queue_depth: 4096
  drop_policy: drop_oldest
rhai:
  max_operations: 10000
  max_call_depth: 32
pools:
  - name: default
    backends:
      - address: "127.0.0.1:5300"
control:
  listen_address: "127.0.0.1:5199"
rules:
  match_mode: first_match
  rules:
    - name: rsrc
      hook: response
      selectors: []
      actions:
        - type: rhai
          value: "{}"
"#,
            script_path.display()
        );
        let cfg = load_yaml(&yaml).unwrap();
        let scripting = compile_from_config(&cfg, Some(&dir)).unwrap();
        let mut host = MockHost {
            id: 53,
            global_query_index: 0,
            qname: "test.example.".into(),
            qtype: 1,
            qclass: 1,
            opcode: 0,
            edns_option_codes: vec![],
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
            phase: ScriptPhase::Response,
            ..Default::default()
        };
        let (_, stats) = run_scripts(
            &scripting,
            &[0],
            &mut host,
            ScriptPhase::Response,
            None,
            None,
            None,
            None,
        );
        assert_eq!(stats.errors, 0);
        assert_eq!(
            host.retry_source_override_v4,
            Some("10.0.0.5".parse().unwrap())
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn script_error_increments_counter() {
        let engine = Engine::new();
        let ast = engine.compile("undefined_fn();").unwrap();
        let effects = Arc::new(Mutex::new(ScriptEffects::default()));
        let txn = RhaiTxn {
            phase: ScriptPhase::Request,
            txn_id: 1,
            global_query_index: 0,
            config_generation: 0,
            rule_name: "test-rule".into(),
            qname: None,
            qtype: None,
            qclass: None,
            opcode: None,
            edns_option_codes: Vec::new(),
            dns_id: 0,
            rcode: None,
            client_addr: "127.0.0.1:53".parse().unwrap(),
            client_protocol: ClientProtocol::Udp,
            listener_label: None,
            received_at: std::time::UNIX_EPOCH,
            selected_pool: None,
            selected_backend: None,
            selected_backend_label: None,
            response_meta: None,
            attempt_count: 0,
            started_at: Instant::now(),
            last_forward_ms: 0,
            answer_source: None,
            cache_instance: None,
            tags_snapshot_bools: HashMap::new(),
            tags_snapshot_strings: HashMap::new(),
            effects: effects.clone(),
        };
        let before = rhai_script_errors_total();
        let mut scope = Scope::new();
        scope.push("txn", txn);
        let _ = engine.run_ast_with_scope(&mut scope, &ast);
        assert!(rhai_script_errors_total() >= before);
    }

    #[test]
    fn blocklist_drop_via_script() {
        let yaml = include_str!("../../../tests/fixtures/config/with-rhai-blocklist.yaml");
        let cfg = load_yaml(yaml).unwrap();
        let base = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/config");
        let scripting = compile_from_config(&cfg, Some(&base)).unwrap();
        let script_id = scripting.rules_scripts[0].script_id;
        let mut host = MockHost {
            id: 3,
            global_query_index: 0,
            qname: "bad.example.".into(),
            qtype: 1,
            qclass: 1,
            opcode: 0,
            edns_option_codes: vec![],
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
            ..Default::default()
        };
        let (outcome, stats) = run_scripts(
            &scripting,
            &[script_id],
            &mut host,
            ScriptPhase::Request,
            None,
            None,
            None,
            None,
        );
        assert_eq!(stats.errors, 0, "script should not error");
        assert_eq!(outcome, ScriptRunOutcome::Drop);
        assert!(host.soft_drop);
    }

    #[test]
    fn servfail_retry_script_sets_pool() {
        let yaml = include_str!("../../../tests/fixtures/config/with-rhai-servfail-retry.yaml");
        let cfg = load_yaml(yaml).unwrap();
        let base = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/config");
        let scripting = compile_from_config(&cfg, Some(&base)).unwrap();
        let script_id = scripting
            .rules_scripts
            .iter()
            .find(|r| r.rule_name == "servfail-retry")
            .unwrap()
            .script_id;
        let mut host = MockHost {
            id: 2,
            global_query_index: 0,
            qname: "test.example.".into(),
            qtype: 1,
            qclass: 1,
            opcode: 0,
            edns_option_codes: vec![],
            dns_id: 1,
            rcode: Some(2),
            pool: Some("primary".into()),
            retry: None,
            dropped: false,
            soft_drop: false,
            source_override_v4: None,
            source_override_v6: None,
            retry_source_override_v4: None,
            retry_source_override_v6: None,
            tags: HashMap::new(),
            tag_strings: HashMap::new(),
            attempts: 1,
            started: Instant::now(),
            last_forward_ms: 0,
            phase: ScriptPhase::Response,
            ..Default::default()
        };
        let (outcome, stats) = run_scripts(
            &scripting,
            &[script_id],
            &mut host,
            ScriptPhase::Response,
            None,
            None,
            None,
            None,
        );
        assert_eq!(stats.errors, 0, "script should not error");
        assert_eq!(outcome, ScriptRunOutcome::Retry);
        assert_eq!(host.retry.as_deref(), Some("secondary"));
    }

    #[test]
    fn request_retry_without_pool() {
        let script = r#"txn.request_retry();"#;
        let dir = std::env::temp_dir().join(format!("conduit-script-retry-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let script_path = dir.join("retry.rhai");
        std::fs::write(&script_path, script).unwrap();
        let yaml = format!(
            r#"
schema_version: 1
listeners:
  threads: 1
  reuse_port: true
  listeners:
    - address: "127.0.0.1:15353"
      protocol: udp
forward:
  outstanding_per_backend: 100
  timeout_ms: 2000
orchestrator:
  max_attempts: 3
  max_txn_duration_ms: 5000
  txn_table_capacity: 1024
events:
  queue_depth: 4096
  drop_policy: drop_oldest
rhai:
  max_operations: 10000
  max_call_depth: 32
pools:
  - name: default
    backends:
      - address: "127.0.0.1:5300"
control:
  listen_address: "127.0.0.1:5199"
rules:
  match_mode: first_match
  rules:
    - name: retry
      hook: response
      selectors: []
      actions:
        - type: rhai
          value: "{}"
"#,
            script_path.display()
        );
        let cfg = load_yaml(&yaml).unwrap();
        let scripting = compile_from_config(&cfg, Some(&dir)).unwrap();
        let script_id = scripting
            .rules_scripts
            .iter()
            .find(|r| r.rule_name == "retry")
            .unwrap()
            .script_id;
        let mut host = MockHost {
            id: 3,
            global_query_index: 0,
            qname: "test.example.".into(),
            qtype: 1,
            qclass: 1,
            opcode: 0,
            edns_option_codes: vec![],
            dns_id: 1,
            rcode: Some(2),
            pool: Some("primary".into()),
            retry: None,
            dropped: false,
            soft_drop: false,
            source_override_v4: None,
            source_override_v6: None,
            retry_source_override_v4: None,
            retry_source_override_v6: None,
            tags: HashMap::new(),
            tag_strings: HashMap::new(),
            attempts: 1,
            started: Instant::now(),
            last_forward_ms: 0,
            phase: ScriptPhase::Response,
            ..Default::default()
        };
        let (outcome, stats) = run_scripts(
            &scripting,
            &[script_id],
            &mut host,
            ScriptPhase::Response,
            None,
            None,
            None,
            None,
        );
        assert_eq!(stats.errors, 0);
        assert_eq!(outcome, ScriptRunOutcome::Retry);
        assert!(host.retry.is_none());
    }

    #[test]
    fn set_pool_via_script() {
        let yaml = include_str!("../../../tests/fixtures/config/with-rhai-minimal.yaml");
        let cfg = load_yaml(yaml).unwrap();
        let base = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/config");
        let scripting = compile_from_config(&cfg, Some(&base)).unwrap();
        let mut host = MockHost {
            id: 1,
            global_query_index: 0,
            qname: "foo.vip.example.".into(),
            qtype: 1,
            qclass: 1,
            opcode: 0,
            edns_option_codes: vec![],
            dns_id: 42,
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
            ..Default::default()
        };
        let ids = vec![0];
        let (outcome, _) = run_scripts(
            &scripting,
            &ids,
            &mut host,
            ScriptPhase::Request,
            None,
            None,
            None,
            None,
        );
        assert_eq!(outcome, ScriptRunOutcome::Ok);
        assert_eq!(host.pool.as_deref(), Some("vip"));
    }

    #[test]
    fn clear_pool_via_script() {
        let yaml = include_str!("../../../tests/fixtures/config/with-rhai-clear-pool.yaml");
        let cfg = load_yaml(yaml).unwrap();
        let base = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/config");
        let scripting = compile_from_config(&cfg, Some(&base)).unwrap();
        let mut host = MockHost {
            id: 1,
            global_query_index: 0,
            qname: "foo.example.".into(),
            qtype: 1,
            qclass: 1,
            opcode: 0,
            edns_option_codes: vec![],
            dns_id: 42,
            rcode: None,
            pool: Some("primary".into()),
            selected_pool: Some("primary".into()),
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
            ..Default::default()
        };
        let ids = vec![0];
        let (outcome, _) = run_scripts(
            &scripting,
            &ids,
            &mut host,
            ScriptPhase::Request,
            None,
            None,
            None,
            None,
        );
        assert_eq!(outcome, ScriptRunOutcome::Ok);
        assert!(host.pool.is_none());
        assert!(host.selected_pool.is_none());
    }

    #[test]
    fn response_in_request_phase_fail_open() {
        let yaml = include_str!("../../../tests/fixtures/config/with-rhai-bad-phase.yaml");
        let cfg = load_yaml(yaml).unwrap();
        let base = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/config");
        let scripting = compile_from_config(&cfg, Some(&base)).unwrap();
        let before = rhai_script_errors_total();
        let mut host = MockHost {
            id: 9,
            global_query_index: 0,
            qname: "test.example.".into(),
            qtype: 1,
            qclass: 1,
            opcode: 0,
            edns_option_codes: vec![],
            dns_id: 1,
            rcode: None,
            pool: Some("default".into()),
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
            ..Default::default()
        };
        let (outcome, stats) = run_scripts(
            &scripting,
            &[0],
            &mut host,
            ScriptPhase::Request,
            None,
            None,
            None,
            None,
        );
        assert_eq!(stats.errors, 1);
        assert_eq!(outcome, ScriptRunOutcome::Ok);
        assert!(!host.dropped);
        assert!(rhai_script_errors_total() > before);
    }

    #[test]
    fn infinite_loop_respects_operation_limit() {
        let yaml = include_str!("../../../tests/fixtures/config/with-rhai-infinite-loop.yaml");
        let cfg = load_yaml(yaml).unwrap();
        let base = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/config");
        let scripting = compile_from_config(&cfg, Some(&base)).unwrap();
        let mut host = MockHost {
            id: 10,
            global_query_index: 0,
            qname: "loop.example.".into(),
            qtype: 1,
            qclass: 1,
            opcode: 0,
            edns_option_codes: vec![],
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
            ..Default::default()
        };
        let (outcome, stats) = run_scripts(
            &scripting,
            &[0],
            &mut host,
            ScriptPhase::Request,
            None,
            None,
            None,
            None,
        );
        assert_eq!(stats.errors, 1);
        assert_eq!(outcome, ScriptRunOutcome::Ok);
        assert!(!host.dropped);
    }

    #[test]
    fn routing_host_calls_count_against_operation_limit() {
        reset_thread_runtime_for_tests();
        let mut engine = Engine::new();
        register_host_api(&mut engine);
        engine.set_max_operations(50);

        let mut pools = HashMap::new();
        pools.insert(
            "primary".to_string(),
            PoolRoutingView {
                configured: true,
                configured_count: 2,
                eligible_count: 2,
                fail_open_active: false,
                min_latency_ewma_ms: None,
                max_outstanding: 0,
            },
        );
        let snapshot = Arc::new(RoutingRuntimeSnapshot::new(1, pools, HashMap::new()));

        let effects = Arc::new(Mutex::new(ScriptEffects::default()));
        let txn = RhaiTxn {
            phase: ScriptPhase::Request,
            txn_id: 1,
            global_query_index: 0,
            config_generation: 0,
            rule_name: "routing-op-budget".into(),
            qname: Some("test.example.".into()),
            qtype: Some(1),
            qclass: Some(1),
            opcode: Some(0),
            edns_option_codes: Vec::new(),
            dns_id: 1,
            rcode: None,
            client_addr: "127.0.0.1:53".parse().unwrap(),
            client_protocol: ClientProtocol::Udp,
            listener_label: None,
            received_at: std::time::UNIX_EPOCH,
            selected_pool: None,
            selected_backend: None,
            selected_backend_label: None,
            response_meta: None,
            attempt_count: 0,
            started_at: Instant::now(),
            last_forward_ms: 0,
            answer_source: None,
            cache_instance: None,
            tags_snapshot_bools: HashMap::new(),
            tags_snapshot_strings: HashMap::new(),
            effects: effects.clone(),
        };
        let mut scope = Scope::new();
        scope.push("txn", txn);
        scope.push("runtime", RuntimeView::new(snapshot));

        let ast = engine
            .compile(r#"while runtime.routing().pool("primary").eligible_count() >= 0 {}"#)
            .unwrap();
        let result = engine.run_ast_with_scope(&mut scope, &ast);
        assert!(result.is_err(), "expected operation limit");
        let msg = result.unwrap_err().to_string().to_lowercase();
        assert!(
            msg.contains("operations") || msg.contains("too many"),
            "expected operation limit error, got: {msg}"
        );
    }

    #[test]
    fn block_hits_records_bounded_metric() {
        let yaml = include_str!("../../../tests/fixtures/config/with-rhai-block-hits.yaml");
        let cfg = load_yaml(yaml).unwrap();
        let base = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/config");
        let scripting = compile_from_config(&cfg, Some(&base)).unwrap();
        let mut host = MockHost {
            id: 11,
            global_query_index: 0,
            qname: "eu.example.".into(),
            qtype: 1,
            qclass: 1,
            opcode: 0,
            edns_option_codes: vec![],
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
            ..Default::default()
        };
        let (_, stats) = run_scripts(
            &scripting,
            &[0],
            &mut host,
            ScriptPhase::Request,
            None,
            None,
            None,
            None,
        );
        assert_eq!(stats.errors, 0);
        assert_eq!(
            stats
                .user_metrics
                .iter()
                .filter(|m| m.name == "block_hits")
                .map(|m| m.delta)
                .sum::<u64>(),
            1
        );
    }

    #[test]
    fn thread_local_engine_reused_for_same_snapshot() {
        reset_thread_runtime_for_tests();

        let yaml = include_str!("../../../tests/fixtures/config/with-rhai-minimal.yaml");
        let cfg = load_yaml(yaml).unwrap();
        let base = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/config");
        let scripting = compile_from_config(&cfg, Some(&base)).unwrap();

        let mut host = MockHost {
            id: 20,
            global_query_index: 0,
            qname: "foo.vip.example.".into(),
            qtype: 1,
            qclass: 1,
            opcode: 0,
            edns_option_codes: vec![],
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
            ..Default::default()
        };

        let (_, _) = run_scripts(
            &scripting,
            &[0],
            &mut host,
            ScriptPhase::Request,
            None,
            None,
            None,
            None,
        );
        assert_eq!(
            thread_runtime_engine_builds(),
            1,
            "first run on this thread should build exactly one engine"
        );
        host.pool = None;
        let (_, _) = run_scripts(
            &scripting,
            &[0],
            &mut host,
            ScriptPhase::Request,
            None,
            None,
            None,
            None,
        );

        assert_eq!(
            thread_runtime_engine_builds(),
            1,
            "second run on the same snapshot generation must not rebuild the engine"
        );
    }

    #[test]
    fn lookup_reflects_snapshot_reload_on_same_thread() {
        reset_thread_runtime_for_tests();

        let dir =
            std::env::temp_dir().join(format!("conduit-script-reload-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let geo_path = dir.join("geo.csv");
        std::fs::write(&geo_path, "qname,region\neu.example.,eu\n").unwrap();

        let yaml = format!(
            r#"
schema_version: 1
listeners:
  threads: 1
  reuse_port: true
  listeners:
    - address: "127.0.0.1:15353"
      protocol: udp
forward:
  outstanding_per_backend: 100
  timeout_ms: 2000
orchestrator:
  max_attempts: 3
  max_txn_duration_ms: 5000
  txn_table_capacity: 1024
events:
  queue_depth: 4096
  drop_policy: drop_oldest
rhai:
  max_operations: 10000
  max_call_depth: 32
  hook_timeout_ms: 50
data_sources:
  - name: geo
    type: csv
    path: "{}"
    key_column: qname
    value_column: region
pools:
  - name: default
    backends:
      - address: "127.0.0.1:5300"
        weight: 100
control:
  listen_address: "127.0.0.1:5199"
rules:
  match_mode: first_match
  rules:
    - name: geo-metrics
      hook: request
      selectors:
        - type: qname_suffix
          value: ".example."
      actions:
        - type: rhai
          value: "PLACEHOLDER_SCRIPT"
"#,
            geo_path.display()
        );

        let script_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures/rhai/block-hits.rhai");
        let yaml = yaml.replace("PLACEHOLDER_SCRIPT", &script_path.display().to_string());

        let cfg = load_yaml(&yaml).unwrap();
        let snap1 = compile_from_config(&cfg, Some(&dir)).unwrap();

        let mut host = MockHost {
            id: 21,
            global_query_index: 0,
            qname: "eu.example.".into(),
            qtype: 1,
            qclass: 1,
            opcode: 0,
            edns_option_codes: vec![],
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
            ..Default::default()
        };
        let (_, stats1) = run_scripts(
            &snap1,
            &[0],
            &mut host,
            ScriptPhase::Request,
            None,
            None,
            None,
            None,
        );
        assert_eq!(
            stats1
                .user_metrics
                .iter()
                .filter(|m| m.name == "block_hits")
                .map(|m| m.delta)
                .sum::<u64>(),
            1
        );

        std::fs::write(&geo_path, "qname,region\neu.example.,us\n").unwrap();
        let snap2 = compile_from_config(&cfg, Some(&dir)).unwrap();
        assert_ne!(snap1.snapshot_generation, snap2.snapshot_generation);

        let (_, stats2) = run_scripts(
            &snap2,
            &[0],
            &mut host,
            ScriptPhase::Request,
            None,
            None,
            None,
            None,
        );
        assert_eq!(stats2.errors, 0);
        assert_eq!(
            stats2
                .user_metrics
                .iter()
                .filter(|m| m.name == "block_hits")
                .map(|m| m.delta)
                .sum::<u64>(),
            1
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn request_response_pair_slow_login_metric() {
        let yaml = include_str!("../../../tests/fixtures/config/with-rhai-slow-login.yaml");
        let cfg = load_yaml(yaml).unwrap();
        let base = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/config");
        let scripting = compile_from_config(&cfg, Some(&base)).unwrap();
        let request_id = scripting
            .rules_scripts
            .iter()
            .find(|r| r.rule_name == "tag-suspicious-login")
            .unwrap()
            .script_id;
        let response_id = scripting
            .rules_scripts
            .iter()
            .find(|r| r.rule_name == "slow-login-alert")
            .unwrap()
            .script_id;

        let mut host = MockHost {
            id: 12,
            global_query_index: 0,
            qname: "login.suspicious.example.".into(),
            qtype: 1,
            qclass: 1,
            opcode: 0,
            edns_option_codes: vec![],
            dns_id: 1,
            rcode: Some(0),
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
            attempts: 1,
            started: Instant::now(),
            last_forward_ms: 600,
            phase: ScriptPhase::Request,
            ..Default::default()
        };
        let (_, _) = run_scripts(
            &scripting,
            &[request_id],
            &mut host,
            ScriptPhase::Request,
            None,
            None,
            None,
            None,
        );
        assert!(host.tags.get("suspicious").copied().unwrap_or(false));

        host.phase = ScriptPhase::Response;
        let (_, stats) = run_scripts(
            &scripting,
            &[response_id],
            &mut host,
            ScriptPhase::Response,
            None,
            None,
            None,
            None,
        );
        assert_eq!(stats.errors, 0, "response script should succeed");
        assert_eq!(
            stats
                .user_metrics
                .iter()
                .filter(|m| m.name == "slow_login")
                .map(|m| m.delta)
                .sum::<u64>(),
            1
        );
    }

    #[test]
    fn last_forward_ms_exposed_to_script() {
        let mut host = MockHost {
            id: 20,
            qname: "slow.example.".into(),
            last_forward_ms: 42,
            phase: ScriptPhase::Response,
            ..Default::default()
        };
        let (outcome, stats) = run_inline_script(
            r#"if txn.last_forward_ms() == 42 { metrics.inc("rtt_ok", 1); }"#,
            &mut host,
        );
        assert_eq!(outcome, ScriptRunOutcome::Ok);
        assert_eq!(stats.errors, 0);
        assert_eq!(
            stats
                .user_metrics
                .iter()
                .filter(|m| m.name == "rtt_ok")
                .map(|m| m.delta)
                .sum::<u64>(),
            1
        );
    }

    #[test]
    fn answer_source_exposed_to_script() {
        let mut host = MockHost {
            answer_source: Some("cache".into()),
            cache_instance: Some("global".into()),
            phase: ScriptPhase::Response,
            ..Default::default()
        };
        let (outcome, stats) = run_inline_script(
            r#"if txn.answer_source() == "cache" && txn.cache_instance() == "global" { metrics.inc("cache_ok", 1); }"#,
            &mut host,
        );
        assert_eq!(outcome, ScriptRunOutcome::Ok);
        assert_eq!(stats.errors, 0);
        assert_eq!(
            stats
                .user_metrics
                .iter()
                .filter(|m| m.name == "cache_ok")
                .map(|m| m.delta)
                .sum::<u64>(),
            1
        );
    }

    #[test]
    fn answer_source_empty_on_request_hook() {
        let mut host = MockHost {
            answer_source: Some("cache".into()),
            phase: ScriptPhase::Request,
            ..Default::default()
        };
        let (outcome, stats) = run_inline_script(
            r#"if txn.answer_source() == "" { metrics.inc("empty_src", 1); }"#,
            &mut host,
        );
        assert_eq!(outcome, ScriptRunOutcome::Ok);
        assert_eq!(stats.errors, 0);
        assert_eq!(
            stats
                .user_metrics
                .iter()
                .filter(|m| m.name == "empty_src")
                .map(|m| m.delta)
                .sum::<u64>(),
            1
        );
    }

    #[test]
    fn set_cache_lookup_eligible_on_request_hook() {
        let mut host = MockHost {
            phase: ScriptPhase::Request,
            ..Default::default()
        };
        let (outcome, stats) =
            run_inline_script(r#"txn.set_cache_lookup_eligible(false);"#, &mut host);
        assert_eq!(outcome, ScriptRunOutcome::Ok);
        assert_eq!(stats.errors, 0);
        assert!(!host.cache_lookup_eligible);
    }

    #[test]
    fn set_cache_lookup_eligible_ignored_on_response_hook() {
        let mut host = MockHost {
            phase: ScriptPhase::Response,
            ..Default::default()
        };
        let (outcome, _) = run_inline_script(r#"txn.set_cache_lookup_eligible(false);"#, &mut host);
        assert_eq!(outcome, ScriptRunOutcome::Ok);
        assert!(host.cache_lookup_eligible);
    }

    fn run_inline_script(script: &str, host: &mut MockHost) -> (ScriptRunOutcome, ScriptRunStats) {
        static RUN: AtomicU64 = AtomicU64::new(0);
        let run_id = RUN.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "conduit-script-tag-{}-{}",
            std::process::id(),
            run_id
        ));
        let _ = std::fs::create_dir_all(&dir);
        let script_path = dir.join("tag.rhai");
        std::fs::write(&script_path, script).unwrap();
        let hook_phase = host.phase();
        let hook_name = match hook_phase {
            ScriptPhase::Request => "request",
            ScriptPhase::Response => "response",
        };
        let yaml = format!(
            r#"
schema_version: 1
listeners:
  threads: 1
  reuse_port: false
  listeners:
    - address: "127.0.0.1:15353"
      protocol: udp
forward:
  outstanding_per_backend: 100
  timeout_ms: 2000
orchestrator:
  max_attempts: 3
  max_txn_duration_ms: 5000
  txn_table_capacity: 1024
events:
  queue_depth: 4096
  drop_policy: drop_oldest
rhai:
  max_operations: 10000
  max_call_depth: 32
pools:
  - name: default
    backends:
      - address: "127.0.0.1:5300"
control:
  listen_address: "127.0.0.1:5199"
rules:
  match_mode: first_match
  rules:
    - name: tag-script
      hook: {hook_name}
      selectors: []
      actions:
        - type: rhai
          value: "{}"
"#,
            script_path.display()
        );
        let cfg = load_yaml(&yaml).unwrap();
        let scripting = compile_from_config(&cfg, Some(&dir)).unwrap();
        let outcome = run_scripts(&scripting, &[0], host, hook_phase, None, None, None, None);
        let _ = std::fs::remove_dir_all(&dir);
        outcome
    }

    #[test]
    fn clear_tag_last_wins_over_set_tag_in_script() {
        let script = r#"txn.set_tag("flag", true); txn.clear_tag("flag");"#;
        let mut host = MockHost {
            id: 30,
            global_query_index: 0,
            qname: "test.example.".into(),
            qtype: 1,
            qclass: 1,
            opcode: 0,
            edns_option_codes: vec![],
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
            ..Default::default()
        };
        let (outcome, stats) = run_inline_script(script, &mut host);
        assert_eq!(stats.errors, 0);
        assert_eq!(outcome, ScriptRunOutcome::Ok);
        assert!(!host.has_tag("flag"));
    }

    #[test]
    fn clear_tag_removes_host_string_tag() {
        let script = r#"txn.clear_tag("tier");"#;
        let mut host = MockHost {
            id: 31,
            global_query_index: 0,
            qname: "test.example.".into(),
            qtype: 1,
            qclass: 1,
            opcode: 0,
            edns_option_codes: vec![],
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
            tag_strings: HashMap::from([("tier".into(), "vip".into())]),
            attempts: 0,
            started: Instant::now(),
            last_forward_ms: 0,
            phase: ScriptPhase::Request,
            ..Default::default()
        };
        let (outcome, stats) = run_inline_script(script, &mut host);
        assert_eq!(stats.errors, 0);
        assert_eq!(outcome, ScriptRunOutcome::Ok);
        assert!(!host.has_tag("tier"));
    }

    #[test]
    fn has_tag_sees_string_snapshot_until_cleared_in_script() {
        let script = r#"if !txn.has_tag("tier") { throw "missing tier"; } txn.clear_tag("tier"); if txn.has_tag("tier") { throw "tier still present"; }"#;
        let mut host = MockHost {
            id: 32,
            global_query_index: 0,
            qname: "test.example.".into(),
            qtype: 1,
            qclass: 1,
            opcode: 0,
            edns_option_codes: vec![],
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
            tag_strings: HashMap::from([("tier".into(), "vip".into())]),
            attempts: 0,
            started: Instant::now(),
            last_forward_ms: 0,
            phase: ScriptPhase::Request,
            ..Default::default()
        };
        let (outcome, stats) = run_inline_script(script, &mut host);
        assert_eq!(stats.errors, 0);
        assert_eq!(outcome, ScriptRunOutcome::Ok);
        assert!(!host.has_tag("tier"));
    }

    #[test]
    fn lookup_unknown_table_increments_script_error_counter() {
        reset_thread_runtime_for_tests();

        let script = r#"let t = "not_in_config"; lookup(t, "key");"#;
        let dir = std::env::temp_dir().join(format!(
            "conduit-script-unknown-table-{}",
            std::process::id()
        ));
        let _ = std::fs::create_dir_all(&dir);
        let script_path = dir.join("unknown.rhai");
        std::fs::write(&script_path, script).unwrap();
        let yaml = format!(
            r#"
schema_version: 1
listeners:
  threads: 1
  reuse_port: false
  listeners:
    - address: "127.0.0.1:15353"
      protocol: udp
forward:
  outstanding_per_backend: 100
  timeout_ms: 2000
orchestrator:
  max_attempts: 3
  max_txn_duration_ms: 5000
  txn_table_capacity: 1024
events:
  queue_depth: 4096
  drop_policy: drop_oldest
rhai:
  max_operations: 10000
  max_call_depth: 32
pools:
  - name: default
    backends:
      - address: "127.0.0.1:5300"
control:
  listen_address: "127.0.0.1:5199"
rules:
  match_mode: first_match
  rules:
    - name: lookup
      hook: request
      selectors: []
      actions:
        - type: rhai
          value: "{}"
"#,
            script_path.display()
        );
        let cfg = load_yaml(&yaml).unwrap();
        let scripting = compile_from_config(&cfg, Some(&dir)).unwrap();
        let builtin = Arc::new(BuiltinRegistry::new(true, BuiltinProfile::Full));
        let encode = |reg: &BuiltinRegistry| {
            let encoder = TextEncoder::new();
            let mut buf = Vec::new();
            encoder.encode(&reg.gather(), &mut buf).unwrap();
            String::from_utf8(buf).unwrap()
        };
        let before_body = encode(builtin.as_ref());
        let mut host = MockHost {
            id: 90,
            global_query_index: 0,
            qname: "test.example.".into(),
            qtype: 1,
            qclass: 1,
            opcode: 0,
            edns_option_codes: vec![],
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
            ..Default::default()
        };
        let (_, stats) = run_scripts(
            &scripting,
            &[0],
            &mut host,
            ScriptPhase::Request,
            None,
            Some(BuiltinProfile::Full),
            Some(builtin.clone()),
            None,
        );
        assert_eq!(stats.errors, 0);
        let after_body = encode(builtin.as_ref());
        assert!(
            !before_body.contains(r#"reason="lookup_unknown_table""#),
            "before:\n{before_body}"
        );
        assert!(
            after_body.contains("conduit_script_errors_total"),
            "after:\n{after_body}"
        );
        assert!(
            after_body.contains(r#"reason="lookup_unknown_table""#),
            "after:\n{after_body}"
        );
        assert!(
            after_body.contains(r#"table="not_in_config""#),
            "after:\n{after_body}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn rhai_sampling_extended_api() {
        reset_thread_runtime_for_tests();
        let mut engine = Engine::new();
        register_host_api(&mut engine);
        let effects = Arc::new(Mutex::new(ScriptEffects::default()));
        let txn = RhaiTxn {
            phase: ScriptPhase::Request,
            txn_id: 8,
            global_query_index: 12,
            config_generation: 0,
            rule_name: "audit-canary".into(),
            qname: Some("login.example.".into()),
            qtype: Some(1),
            qclass: Some(1),
            opcode: Some(0),
            edns_option_codes: Vec::new(),
            dns_id: 1,
            rcode: None,
            client_addr: "127.0.0.1:53".parse().unwrap(),
            client_protocol: ClientProtocol::Udp,
            listener_label: None,
            received_at: std::time::UNIX_EPOCH,
            selected_pool: None,
            selected_backend: None,
            selected_backend_label: None,
            response_meta: None,
            attempt_count: 1,
            started_at: Instant::now(),
            last_forward_ms: 0,
            answer_source: None,
            cache_instance: None,
            tags_snapshot_bools: HashMap::new(),
            tags_snapshot_strings: HashMap::new(),
            effects: effects.clone(),
        };
        let mut scope = Scope::new();
        scope.push("txn", txn);
        assert!(engine
            .eval_with_scope::<bool>(&mut scope, "txn.every_nth_worker(4)")
            .unwrap());
        assert!(!engine
            .eval_with_scope::<bool>(&mut scope, "txn.every_nth_worker(3)")
            .unwrap());
        assert!(engine
            .eval_with_scope::<bool>(&mut scope, "txn.every_nth_global(4)")
            .unwrap());
        assert_eq!(
            engine
                .eval_with_scope::<String>(&mut scope, "txn.rule_name()")
                .unwrap(),
            "audit-canary"
        );
        let keyed = hash_sample_keyed(8, 0.10, Some("audit-canary"));
        assert_eq!(
            engine
                .eval_with_scope::<bool>(&mut scope, "txn.sample_percent_for_rule(10.0)")
                .unwrap(),
            keyed
        );
        let qname_keyed = hash_sample_keyed(8, 0.10, Some("login.example."));
        assert_eq!(
            engine
                .eval_with_scope::<bool>(&mut scope, "txn.sample_percent_for_qname(10.0)")
                .unwrap(),
            qname_keyed
        );
    }

    #[test]
    fn host_context_and_introspection_apis() {
        reset_thread_runtime_for_tests();
        let mut engine = Engine::new();
        register_host_api(&mut engine);
        let effects = Arc::new(Mutex::new(ScriptEffects::default()));
        let received = UNIX_EPOCH + std::time::Duration::from_secs(86_400 + 15_360);
        let txn = RhaiTxn {
            phase: ScriptPhase::Request,
            txn_id: 42,
            global_query_index: 0,
            config_generation: 7,
            rule_name: "test".into(),
            qname: Some("example.com.".into()),
            qtype: Some(1),
            qclass: Some(1),
            opcode: Some(0),
            edns_option_codes: Vec::new(),
            dns_id: 1,
            rcode: None,
            client_addr: "192.0.2.1:53000".parse().unwrap(),
            client_protocol: ClientProtocol::Tcp,
            listener_label: Some("127.0.0.1:53".into()),
            received_at: received,
            selected_pool: Some("vip".into()),
            selected_backend: Some("198.51.100.1:53".parse().unwrap()),
            selected_backend_label: Some("vip-east".into()),
            response_meta: None,
            attempt_count: 0,
            started_at: Instant::now(),
            last_forward_ms: 0,
            answer_source: None,
            cache_instance: None,
            tags_snapshot_bools: HashMap::new(),
            tags_snapshot_strings: HashMap::new(),
            effects: effects.clone(),
        };
        let mut scope = Scope::new();
        scope.push("txn", txn);
        assert_eq!(
            engine
                .eval_with_scope::<i64>(&mut scope, "txn.txn_id()")
                .unwrap(),
            42
        );
        assert_eq!(
            engine
                .eval_with_scope::<i64>(&mut scope, "txn.config_generation()")
                .unwrap(),
            7
        );
        assert_eq!(
            engine
                .eval_with_scope::<String>(&mut scope, "txn.client_ip()")
                .unwrap(),
            "192.0.2.1"
        );
        assert_eq!(
            engine
                .eval_with_scope::<String>(&mut scope, "txn.client_protocol()")
                .unwrap(),
            "tcp"
        );
        assert_eq!(
            engine
                .eval_with_scope::<String>(&mut scope, "txn.listener()")
                .unwrap(),
            "127.0.0.1:53"
        );
        assert_eq!(
            engine
                .eval_with_scope::<String>(&mut scope, "txn.selected_backend()")
                .unwrap(),
            "198.51.100.1:53"
        );
        assert_eq!(
            engine
                .eval_with_scope::<String>(&mut scope, "txn.selected_backend_name()")
                .unwrap(),
            "vip-east"
        );
        assert_eq!(
            engine
                .eval_with_scope::<i64>(&mut scope, "txn.utc_hour()")
                .unwrap(),
            4
        );
    }

    #[test]
    fn response_map_includes_path_and_wire_meta() {
        reset_thread_runtime_for_tests();
        let mut engine = Engine::new();
        register_host_api(&mut engine);
        let effects = Arc::new(Mutex::new(ScriptEffects::default()));
        let txn = RhaiTxn {
            phase: ScriptPhase::Response,
            txn_id: 1,
            global_query_index: 0,
            config_generation: 0,
            rule_name: "r".into(),
            qname: Some("example.com.".into()),
            qtype: Some(1),
            qclass: Some(1),
            opcode: Some(0),
            edns_option_codes: Vec::new(),
            dns_id: 1,
            rcode: Some(0),
            client_addr: "127.0.0.1:53".parse().unwrap(),
            client_protocol: ClientProtocol::Udp,
            listener_label: None,
            received_at: UNIX_EPOCH,
            selected_pool: Some("default".into()),
            selected_backend: Some("198.51.100.1:53".parse().unwrap()),
            selected_backend_label: None,
            response_meta: Some(ResponseWireMeta {
                answer_count: 2,
                authority_count: 0,
                additional_count: 1,
                truncated: false,
                authoritative: true,
            }),
            attempt_count: 1,
            started_at: Instant::now(),
            last_forward_ms: 12,
            answer_source: None,
            cache_instance: None,
            tags_snapshot_bools: HashMap::new(),
            tags_snapshot_strings: HashMap::new(),
            effects: effects.clone(),
        };
        let mut scope = Scope::new();
        scope.push("txn", txn);
        assert!(engine
            .eval_with_scope::<bool>(&mut scope, r#"txn.response()?.authoritative == true"#)
            .unwrap());
        assert_eq!(
            engine
                .eval_with_scope::<i64>(&mut scope, "txn.response_answer_count()")
                .unwrap(),
            2
        );
        assert!(engine
            .eval_with_scope::<bool>(
                &mut scope,
                r#"txn.response()?.backend == "198.51.100.1:53""#
            )
            .unwrap());
        // With no configured backend name, `backend_name` falls back to the address.
        assert!(engine
            .eval_with_scope::<bool>(
                &mut scope,
                r#"txn.response()?.backend_name == "198.51.100.1:53""#
            )
            .unwrap());
        assert_eq!(
            engine
                .eval_with_scope::<String>(&mut scope, "txn.selected_backend_name()")
                .unwrap(),
            "198.51.100.1:53"
        );
    }
}
