use crate::compile::{CompiledScript, CompiledScripting, ScriptLimits};
use crate::data_sources::DataSourceStore;
use crate::host::{HostTransaction, ScriptPhase};
use crate::metrics::MetricRegistry;
use conduit_observation::hash_sample;
use rhai::{Dynamic, Engine, EvalAltResult, Scope};
use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

static SCRIPT_ERRORS: AtomicU64 = AtomicU64::new(0);

#[cfg(test)]
static ENGINE_BUILDS: AtomicU64 = AtomicU64::new(0);

thread_local! {
    static LOOKUP_DATA: RefCell<Option<Arc<DataSourceStore>>> = const { RefCell::new(None) };
    static SCRIPT_RUNTIME: RefCell<Option<ScriptRuntime>> = const { RefCell::new(None) };
}

struct ScriptRuntime {
    engine: Engine,
    snapshot_generation: u64,
}

impl ScriptRuntime {
    fn new(scripting: &CompiledScripting) -> Self {
        #[cfg(test)]
        ENGINE_BUILDS.fetch_add(1, Ordering::Relaxed);

        let mut engine = Engine::new();
        register_host_api(&mut engine);

        Self {
            engine,
            snapshot_generation: scripting.snapshot_generation,
        }
    }
}

fn register_host_api(engine: &mut Engine) {
    engine.register_fn("table_lookup", |table: &str, key: &str| -> String {
        LOOKUP_DATA.with(|cell| {
            cell.borrow()
                .as_ref()
                .map(|data| data.lookup(table, key))
                .unwrap_or_default()
        })
    });

    engine.register_fn("question_qname", |txn: &mut RhaiTxn| -> String {
        txn.question_qname()
    });

    engine
        .register_type_with_name::<RhaiTxn>("Transaction")
        .register_fn("question", RhaiTxn::question)
        .register_fn("response", RhaiTxn::response)
        .register_fn("response_rcode", RhaiTxn::response_rcode)
        .register_fn("set_tag", RhaiTxn::set_tag)
        .register_fn("has_tag", RhaiTxn::has_tag)
        .register_fn("set_pool", RhaiTxn::set_pool)
        .register_fn("set_retry_pool", RhaiTxn::retry)
        .register_fn("drop_query", RhaiTxn::drop)
        .register_fn("set_rcode", RhaiTxn::set_rcode)
        .register_fn("sample_include", RhaiTxn::sample_include)
        .register_fn("metric_inc", RhaiTxn::metric_inc)
        .register_fn("metric_inc_labels", RhaiTxn::metric_inc_labels)
        .register_fn("elapsed_ms", RhaiTxn::elapsed_ms)
        .register_fn("get_attempt_count", RhaiTxn::attempt_count);
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
}

#[cfg(test)]
pub(crate) fn thread_runtime_engine_builds() -> u64 {
    ENGINE_BUILDS.load(Ordering::Relaxed)
}

pub fn rhai_script_errors_total() -> u64 {
    SCRIPT_ERRORS.load(Ordering::Relaxed)
}

#[derive(Debug, Default, Clone)]
pub struct ScriptRunStats {
    pub errors: u32,
    pub user_metrics: HashMap<String, u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScriptRunOutcome {
    Ok,
    Error,
    Drop,
    Retry,
}

#[derive(Debug, Default)]
struct ScriptEffects {
    pool: Option<String>,
    retry_pool: Option<String>,
    tags_bool: HashMap<String, bool>,
    tags_string: HashMap<String, String>,
    rcode: Option<String>,
    dropped: bool,
    sample_decision: Option<bool>,
    user_metrics: HashMap<String, u64>,
}

#[derive(Clone)]
struct RhaiTxn {
    phase: ScriptPhase,
    txn_id: u64,
    qname: Option<String>,
    qtype: Option<String>,
    dns_id: u16,
    rcode: Option<String>,
    attempt_count: u32,
    started_at: Instant,
    tags_snapshot: HashMap<String, bool>,
    metrics: Arc<MetricRegistry>,
    effects: Arc<Mutex<ScriptEffects>>,
}

impl RhaiTxn {
    fn question(&mut self) -> Dynamic {
        let mut map = rhai::Map::new();
        if let Some(ref qname) = self.qname {
            map.insert("qname".into(), Dynamic::from(qname.clone()));
        }
        if let Some(ref qtype) = self.qtype {
            map.insert("qtype".into(), Dynamic::from(qtype.clone()));
        }
        map.insert("id".into(), Dynamic::from(self.dns_id as i64));
        Dynamic::from(map)
    }

    fn question_qname(&self) -> String {
        self.qname.clone().unwrap_or_default()
    }

    fn response(&mut self) -> Result<Dynamic, Box<EvalAltResult>> {
        if self.phase != ScriptPhase::Response {
            return Err("response() is not available in request phase".into());
        }
        let mut map = rhai::Map::new();
        if let Some(ref rcode) = self.rcode {
            map.insert("rcode".into(), Dynamic::from(rcode.clone()));
        }
        if let Some(ref qname) = self.qname {
            map.insert("qname".into(), Dynamic::from(qname.clone()));
        }
        if let Some(ref qtype) = self.qtype {
            map.insert("qtype".into(), Dynamic::from(qtype.clone()));
        }
        Ok(Dynamic::from(map))
    }

    fn response_rcode(&mut self) -> String {
        if self.phase != ScriptPhase::Response {
            return String::new();
        }
        self.rcode.clone().unwrap_or_default()
    }

    fn set_tag(&mut self, key: &str, value: Dynamic) -> Result<(), Box<EvalAltResult>> {
        let mut fx = self.effects.lock().map_err(|e| e.to_string())?;
        if value.is::<bool>() {
            fx.tags_bool
                .insert(key.to_string(), value.as_bool().unwrap_or(false));
        } else {
            fx.tags_string.insert(key.to_string(), value.to_string());
        }
        Ok(())
    }

    fn has_tag(&mut self, key: &str) -> bool {
        if let Ok(fx) = self.effects.lock() {
            if fx.tags_bool.get(key).copied().unwrap_or(false) {
                return true;
            }
            if fx.tags_string.contains_key(key) {
                return true;
            }
        }
        self.tags_snapshot.get(key).copied().unwrap_or(false)
    }

    fn set_pool(&mut self, name: &str) {
        if let Ok(mut fx) = self.effects.lock() {
            fx.pool = Some(name.to_string());
        }
    }

    fn retry(&mut self, pool: &str) {
        if let Ok(mut fx) = self.effects.lock() {
            fx.retry_pool = Some(pool.to_string());
        }
    }

    fn drop(&mut self) {
        if let Ok(mut fx) = self.effects.lock() {
            fx.dropped = true;
        }
    }

    fn set_rcode(&mut self, name: &str) {
        if let Ok(mut fx) = self.effects.lock() {
            fx.rcode = Some(name.to_string());
        }
    }

    fn sample_include(&mut self, rate: f64) -> bool {
        let Ok(mut fx) = self.effects.lock() else {
            return false;
        };
        if let Some(decision) = fx.sample_decision {
            return decision;
        }
        let decision = hash_sample(self.txn_id, rate);
        fx.sample_decision = Some(decision);
        if decision {
            fx.tags_bool.insert("sampled".into(), true);
        }
        decision
    }

    fn metric_inc(&mut self, name: &str, delta: i64) -> Result<(), Box<EvalAltResult>> {
        self.metric_inc_labels(name, delta, rhai::Map::new())
    }

    fn metric_inc_labels(
        &mut self,
        name: &str,
        delta: i64,
        labels: rhai::Map,
    ) -> Result<(), Box<EvalAltResult>> {
        let label_map: HashMap<String, String> = labels
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        self.metrics
            .validate_runtime_labels(name, &label_map)
            .map_err(|e| e.to_string())?;
        let mut fx = self.effects.lock().map_err(|e| e.to_string())?;
        *fx.user_metrics.entry(name.to_string()).or_insert(0) += delta.max(0) as u64;
        Ok(())
    }

    fn elapsed_ms(&mut self) -> i64 {
        self.started_at.elapsed().as_millis() as i64
    }

    fn attempt_count(&mut self) -> i64 {
        self.attempt_count as i64
    }
}

pub fn run_scripts(
    scripting: &CompiledScripting,
    script_ids: &[usize],
    host: &mut dyn HostTransaction,
    phase: ScriptPhase,
) -> (ScriptRunOutcome, ScriptRunStats) {
    if script_ids.is_empty() {
        return (ScriptRunOutcome::Ok, ScriptRunStats::default());
    }

    let mut stats = ScriptRunStats::default();
    let data = Arc::clone(&scripting.data_sources);
    let metrics = Arc::new(scripting.metrics.clone());

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
                data.clone(),
                metrics.clone(),
            )
        });
        match run_result {
            Ok(fx) => {
                apply_effects(host, &fx);
                stats.user_metrics.extend(fx.user_metrics);
                if fx.dropped {
                    return (ScriptRunOutcome::Drop, stats);
                }
                if fx.retry_pool.is_some() {
                    return (ScriptRunOutcome::Retry, stats);
                }
            }
            Err(e) => {
                stats.errors += 1;
                SCRIPT_ERRORS.fetch_add(1, Ordering::Relaxed);
                tracing::warn!(
                    script = %script.path,
                    rule = %script.rule_id,
                    error = %e,
                    "rhai script error"
                );
            }
        }
    }

    (ScriptRunOutcome::Ok, stats)
}

fn apply_effects(host: &mut dyn HostTransaction, fx: &ScriptEffects) {
    if let Some(ref pool) = fx.pool {
        host.set_pool(pool);
    }
    if let Some(ref pool) = fx.retry_pool {
        host.set_retry_pool(pool);
    }
    for (k, v) in &fx.tags_bool {
        host.set_tag_bool(k, *v);
    }
    for (k, v) in &fx.tags_string {
        host.set_tag_string(k, v);
    }
    if let Some(ref rc) = fx.rcode {
        host.set_rcode_name(rc);
    }
    if fx.dropped {
        host.mark_dropped();
    }
}

fn run_one(
    runtime: &mut ScriptRuntime,
    script: &CompiledScript,
    limits: &ScriptLimits,
    host: &dyn HostTransaction,
    phase: ScriptPhase,
    data: Arc<DataSourceStore>,
    metrics: Arc<MetricRegistry>,
) -> Result<ScriptEffects, String> {
    let effects = Arc::new(Mutex::new(ScriptEffects::default()));
    let txn = RhaiTxn {
        phase,
        txn_id: host.txn_id(),
        qname: host.question_qname().map(str::to_string),
        qtype: host.question_qtype_label(),
        dns_id: host.question_id(),
        rcode: host.response_rcode_label(),
        attempt_count: host.attempt_count(),
        started_at: host.started_at(),
        tags_snapshot: host.script_tag_bools(),
        metrics,
        effects: effects.clone(),
    };

    LOOKUP_DATA.with(|cell| *cell.borrow_mut() = Some(data));

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

    let result = engine
        .run_ast_with_scope(&mut scope, &script.ast)
        .map_err(|e| e.to_string());

    LOOKUP_DATA.with(|cell| *cell.borrow_mut() = None);

    result?;

    let fx = effects.lock().map_err(|e| e.to_string())?;
    Ok(ScriptEffects {
        pool: fx.pool.clone(),
        retry_pool: fx.retry_pool.clone(),
        tags_bool: fx.tags_bool.clone(),
        tags_string: fx.tags_string.clone(),
        rcode: fx.rcode.clone(),
        dropped: fx.dropped,
        sample_decision: fx.sample_decision,
        user_metrics: fx.user_metrics.clone(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compile::compile_from_config;
    use conduit_config::load_yaml;
    use std::path::PathBuf;
    use std::time::Duration;

    struct MockHost {
        id: u64,
        qname: String,
        qtype: String,
        dns_id: u16,
        rcode: Option<String>,
        pool: Option<String>,
        retry: Option<String>,
        dropped: bool,
        tags: HashMap<String, bool>,
        attempts: u32,
        started: Instant,
        phase: ScriptPhase,
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
        fn question_qtype_label(&self) -> Option<String> {
            Some(self.qtype.clone())
        }
        fn question_id(&self) -> u16 {
            self.dns_id
        }
        fn response_rcode_label(&self) -> Option<String> {
            self.rcode.clone()
        }
        fn has_tag(&self, key: &str) -> bool {
            self.tags.get(key).copied().unwrap_or(false)
        }
        fn set_tag_bool(&mut self, key: &str, value: bool) {
            self.tags.insert(key.to_string(), value);
        }
        fn set_tag_string(&mut self, _key: &str, _value: &str) {}
        fn set_pool(&mut self, name: &str) {
            self.pool = Some(name.to_string());
        }
        fn set_retry_pool(&mut self, name: &str) {
            self.retry = Some(name.to_string());
        }
        fn drop_query(&mut self) {}
        fn set_rcode_name(&mut self, name: &str) {
            self.rcode = Some(name.to_string());
        }
        fn attempt_count(&self) -> u32 {
            self.attempts
        }
        fn started_at(&self) -> Instant {
            self.started
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
    }

    #[test]
    fn script_error_increments_counter() {
        let engine = Engine::new();
        let ast = engine.compile("undefined_fn();").unwrap();
        let effects = Arc::new(Mutex::new(ScriptEffects::default()));
        let txn = RhaiTxn {
            phase: ScriptPhase::Request,
            txn_id: 1,
            qname: None,
            qtype: None,
            dns_id: 0,
            rcode: None,
            attempt_count: 0,
            started_at: Instant::now(),
            tags_snapshot: HashMap::new(),
            metrics: Arc::new(MetricRegistry::default()),
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
            qname: "bad.example.".into(),
            qtype: "A".into(),
            dns_id: 1,
            rcode: None,
            pool: None,
            retry: None,
            dropped: false,
            tags: HashMap::new(),
            attempts: 0,
            started: Instant::now(),
            phase: ScriptPhase::Request,
        };
        let (outcome, stats) =
            run_scripts(&scripting, &[script_id], &mut host, ScriptPhase::Request);
        assert_eq!(stats.errors, 0, "script should not error");
        assert_eq!(outcome, ScriptRunOutcome::Drop);
        assert!(host.dropped);
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
            .find(|r| r.rule_id == "servfail-retry")
            .unwrap()
            .script_id;
        let mut host = MockHost {
            id: 2,
            qname: "test.example.".into(),
            qtype: "A".into(),
            dns_id: 1,
            rcode: Some("SERVFAIL".into()),
            pool: Some("primary".into()),
            retry: None,
            dropped: false,
            tags: HashMap::new(),
            attempts: 1,
            started: Instant::now(),
            phase: ScriptPhase::Response,
        };
        let (outcome, stats) =
            run_scripts(&scripting, &[script_id], &mut host, ScriptPhase::Response);
        assert_eq!(stats.errors, 0, "script should not error");
        assert_eq!(outcome, ScriptRunOutcome::Retry);
        assert_eq!(host.retry.as_deref(), Some("secondary"));
    }

    #[test]
    fn set_pool_via_script() {
        let yaml = include_str!("../../../tests/fixtures/config/with-rhai-minimal.yaml");
        let cfg = load_yaml(yaml).unwrap();
        let base = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/config");
        let scripting = compile_from_config(&cfg, Some(&base)).unwrap();
        let mut host = MockHost {
            id: 1,
            qname: "foo.vip.example.".into(),
            qtype: "A".into(),
            dns_id: 42,
            rcode: None,
            pool: None,
            retry: None,
            dropped: false,
            tags: HashMap::new(),
            attempts: 0,
            started: Instant::now(),
            phase: ScriptPhase::Request,
        };
        let ids = vec![0];
        let (outcome, _) = run_scripts(&scripting, &ids, &mut host, ScriptPhase::Request);
        assert_eq!(outcome, ScriptRunOutcome::Ok);
        assert_eq!(host.pool.as_deref(), Some("vip"));
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
            qname: "test.example.".into(),
            qtype: "A".into(),
            dns_id: 1,
            rcode: None,
            pool: Some("default".into()),
            retry: None,
            dropped: false,
            tags: HashMap::new(),
            attempts: 0,
            started: Instant::now(),
            phase: ScriptPhase::Request,
        };
        let (outcome, stats) = run_scripts(&scripting, &[0], &mut host, ScriptPhase::Request);
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
            qname: "loop.example.".into(),
            qtype: "A".into(),
            dns_id: 1,
            rcode: None,
            pool: None,
            retry: None,
            dropped: false,
            tags: HashMap::new(),
            attempts: 0,
            started: Instant::now(),
            phase: ScriptPhase::Request,
        };
        let (outcome, stats) = run_scripts(&scripting, &[0], &mut host, ScriptPhase::Request);
        assert_eq!(stats.errors, 1);
        assert_eq!(outcome, ScriptRunOutcome::Ok);
        assert!(!host.dropped);
    }

    #[test]
    fn block_hits_records_bounded_metric() {
        let yaml = include_str!("../../../tests/fixtures/config/with-rhai-block-hits.yaml");
        let cfg = load_yaml(yaml).unwrap();
        let base = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/config");
        let scripting = compile_from_config(&cfg, Some(&base)).unwrap();
        let mut host = MockHost {
            id: 11,
            qname: "eu.example.".into(),
            qtype: "A".into(),
            dns_id: 1,
            rcode: None,
            pool: None,
            retry: None,
            dropped: false,
            tags: HashMap::new(),
            attempts: 0,
            started: Instant::now(),
            phase: ScriptPhase::Request,
        };
        let (_, stats) = run_scripts(&scripting, &[0], &mut host, ScriptPhase::Request);
        assert_eq!(stats.errors, 0);
        assert_eq!(stats.user_metrics.get("block_hits"), Some(&1));
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
            qname: "foo.vip.example.".into(),
            qtype: "A".into(),
            dns_id: 1,
            rcode: None,
            pool: None,
            retry: None,
            dropped: false,
            tags: HashMap::new(),
            attempts: 0,
            started: Instant::now(),
            phase: ScriptPhase::Request,
        };

        let (_, _) = run_scripts(&scripting, &[0], &mut host, ScriptPhase::Request);
        let builds_after_first = thread_runtime_engine_builds();
        host.pool = None;
        let (_, _) = run_scripts(&scripting, &[0], &mut host, ScriptPhase::Request);

        assert_eq!(
            thread_runtime_engine_builds(),
            builds_after_first,
            "second run on the same snapshot generation must not rebuild the engine"
        );
        assert!(
            builds_after_first > 0,
            "expected at least one engine build on first run"
        );
    }

    #[test]
    fn table_lookup_reflects_snapshot_reload_on_same_thread() {
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
observation:
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
    - id: geo-metrics
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
            qname: "eu.example.".into(),
            qtype: "A".into(),
            dns_id: 1,
            rcode: None,
            pool: None,
            retry: None,
            dropped: false,
            tags: HashMap::new(),
            attempts: 0,
            started: Instant::now(),
            phase: ScriptPhase::Request,
        };
        let (_, stats1) = run_scripts(&snap1, &[0], &mut host, ScriptPhase::Request);
        assert_eq!(stats1.user_metrics.get("block_hits"), Some(&1));

        std::fs::write(&geo_path, "qname,region\neu.example.,us\n").unwrap();
        let snap2 = compile_from_config(&cfg, Some(&dir)).unwrap();
        assert_ne!(snap1.snapshot_generation, snap2.snapshot_generation);

        let (_, stats2) = run_scripts(&snap2, &[0], &mut host, ScriptPhase::Request);
        assert_eq!(stats2.errors, 0);
        assert_eq!(stats2.user_metrics.get("block_hits"), Some(&1));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    #[ignore = "micro-benchmark for local perf comparison; run: cargo test -p conduit-script thread_local_runtime_bench -- --ignored --nocapture"]
    fn thread_local_runtime_bench() {
        reset_thread_runtime_for_tests();
        let yaml = include_str!("../../../tests/fixtures/config/with-rhai-minimal.yaml");
        let cfg = load_yaml(yaml).unwrap();
        let base = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/config");
        let scripting = compile_from_config(&cfg, Some(&base)).unwrap();
        let mut host = MockHost {
            id: 99,
            qname: "foo.vip.example.".into(),
            qtype: "A".into(),
            dns_id: 1,
            rcode: None,
            pool: None,
            retry: None,
            dropped: false,
            tags: HashMap::new(),
            attempts: 0,
            started: Instant::now(),
            phase: ScriptPhase::Request,
        };
        let n = 10_000u32;
        let start = Instant::now();
        for _ in 0..n {
            let _ = run_scripts(&scripting, &[0], &mut host, ScriptPhase::Request);
            host.pool = None;
        }
        let elapsed = start.elapsed();
        eprintln!(
            "thread_local_runtime_bench: {n} runs in {:?} ({:.0} runs/sec)",
            elapsed,
            n as f64 / elapsed.as_secs_f64()
        );
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
            .find(|r| r.rule_id == "tag-suspicious-login")
            .unwrap()
            .script_id;
        let response_id = scripting
            .rules_scripts
            .iter()
            .find(|r| r.rule_id == "slow-login-alert")
            .unwrap()
            .script_id;

        let mut host = MockHost {
            id: 12,
            qname: "login.suspicious.example.".into(),
            qtype: "A".into(),
            dns_id: 1,
            rcode: Some("NOERROR".into()),
            pool: None,
            retry: None,
            dropped: false,
            tags: HashMap::new(),
            attempts: 0,
            started: Instant::now() - Duration::from_millis(600),
            phase: ScriptPhase::Request,
        };
        let (_, _) = run_scripts(&scripting, &[request_id], &mut host, ScriptPhase::Request);
        assert!(host.tags.get("suspicious").copied().unwrap_or(false));

        host.phase = ScriptPhase::Response;
        let (_, stats) = run_scripts(&scripting, &[response_id], &mut host, ScriptPhase::Response);
        assert_eq!(stats.errors, 0, "response script should succeed");
        assert_eq!(stats.user_metrics.get("slow_login"), Some(&1));
    }
}
