use crate::capability_scan::script_needs_response_wire_meta;
use crate::data_sources::{load_data_sources, DataSourceLimits, DataSourceStore};
use crate::error::ScriptError;
use crate::host::ScriptPhase;
use crate::lookup_scan::validate_lookup_literals;
use crate::metrics::{scan_metric_sites, MetricRegistry};
use conduit_metrics::{
    check_consumer_dependencies, resolve_metrics_plan, ConsumerKind, MetricConsumerGraph,
    MetricConsumerRef,
};
use conduit_proto::config::{Config, RhaiConfig, Rule};
use conduit_proto::paths::resolve_config_path;
use rhai::AST;
use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

static SNAPSHOT_GENERATION: AtomicU64 = AtomicU64::new(0);

pub const DEFAULT_HOOK_TIMEOUT_MS: u32 = 50;
pub const DEFAULT_MAX_OPERATIONS: u64 = 10_000;
pub const DEFAULT_MAX_CALL_DEPTH: u32 = 32;

#[derive(Debug, Clone)]
pub struct ScriptLimits {
    pub max_operations: u64,
    pub max_call_depth: u32,
    pub hook_timeout_ms: u32,
}

impl ScriptLimits {
    pub fn from_config(rhai: Option<&RhaiConfig>) -> Self {
        let Some(r) = rhai else {
            return Self {
                max_operations: DEFAULT_MAX_OPERATIONS,
                max_call_depth: DEFAULT_MAX_CALL_DEPTH,
                hook_timeout_ms: DEFAULT_HOOK_TIMEOUT_MS,
            };
        };
        Self {
            max_operations: if r.max_operations == 0 {
                DEFAULT_MAX_OPERATIONS
            } else {
                r.max_operations
            },
            max_call_depth: if r.max_call_depth == 0 {
                DEFAULT_MAX_CALL_DEPTH
            } else {
                r.max_call_depth
            },
            hook_timeout_ms: if r.hook_timeout_ms == 0 {
                DEFAULT_HOOK_TIMEOUT_MS
            } else {
                r.hook_timeout_ms
            },
        }
    }
}

#[derive(Debug, Clone)]
pub struct ScriptRef {
    pub rule_name: String,
    pub hook: ScriptPhase,
    pub path: String,
    pub script_id: usize,
}

#[derive(Debug)]
pub struct CompiledScript {
    pub path: String,
    pub rule_name: String,
    pub hook: ScriptPhase,
    pub ast: AST,
}

#[derive(Debug)]
pub struct CompiledScripting {
    pub scripts: Vec<CompiledScript>,
    pub script_index: HashMap<(String, String), usize>,
    pub data_sources: Arc<DataSourceStore>,
    pub snapshot_generation: u64,
    pub limits: ScriptLimits,
    pub metrics: MetricRegistry,
    /// Static metric consumer sites from Rhai (and future stub sources).
    pub metric_consumers: MetricConsumerGraph,
    pub rules_scripts: Vec<ScriptRef>,
    /// When true, forward stage parses upstream response wire for section/header metadata.
    pub needs_response_wire_meta: bool,
}

impl CompiledScripting {
    pub fn script_ids_for_rule(&self, rule_name: &str, hook: ScriptPhase) -> Vec<usize> {
        self.rules_scripts
            .iter()
            .filter(|r| r.rule_name == rule_name && r.hook == hook)
            .map(|r| r.script_id)
            .collect()
    }

    pub fn is_empty(&self) -> bool {
        self.scripts.is_empty()
    }
}

pub fn compile_from_config(
    config: &Config,
    base_dir: Option<&Path>,
) -> Result<CompiledScripting, ScriptError> {
    let limits = ScriptLimits::from_config(config.rhai.as_ref());
    let data_source_limits = DataSourceLimits::from_config(config.data_source_limits.as_ref());
    let data_sources = Arc::new(load_data_sources(
        &config.data_sources,
        base_dir,
        &data_source_limits,
    )?);
    let snapshot_generation = SNAPSHOT_GENERATION.fetch_add(1, Ordering::Relaxed) + 1;

    let mut scripting = CompiledScripting {
        scripts: Vec::new(),
        script_index: HashMap::new(),
        data_sources,
        snapshot_generation,
        limits,
        metrics: MetricRegistry::default(),
        metric_consumers: MetricConsumerGraph::new(),
        rules_scripts: Vec::new(),
        needs_response_wire_meta: false,
    };

    let Some(rules) = config.rules.as_ref() else {
        scripting.metric_consumers.extend_from_stub_registries();
        return Ok(scripting);
    };

    for rule in &rules.rules {
        compile_rule_scripts(rule, base_dir, &mut scripting)?;
    }

    scripting
        .metrics
        .apply_user_metric_exports(config.metrics.as_ref())?;

    scripting.metric_consumers.extend_from_stub_registries();

    let plan = match resolve_metrics_plan(config.metrics.as_ref()) {
        Ok(r) => r.plan,
        Err(errs) => {
            return Err(ScriptError::Metric {
                name: String::new(),
                message: errs.join("; "),
            });
        }
    };
    let consumer_errs = check_consumer_dependencies(&scripting.metric_consumers, &plan);
    if !consumer_errs.is_empty() {
        return Err(ScriptError::ConsumerDependency(consumer_errs.join("\n\n")));
    }

    Ok(scripting)
}

fn compile_rule_scripts(
    rule: &Rule,
    base_dir: Option<&Path>,
    scripting: &mut CompiledScripting,
) -> Result<(), ScriptError> {
    let hook = if rule.hook == "response" {
        ScriptPhase::Response
    } else {
        ScriptPhase::Request
    };

    for action in &rule.actions {
        if action.r#type != "rhai" {
            continue;
        }
        if action.value.is_empty() {
            return Err(ScriptError::Rule {
                rule_name: rule.name.clone(),
                message: "rhai action requires script path in value".into(),
            });
        }
        let resolved = resolve_config_path(base_dir, &action.value);
        let path_key = resolved.display().to_string();
        let script_id = if let Some(&id) = scripting
            .script_index
            .get(&(rule.name.clone(), path_key.clone()))
        {
            id
        } else {
            let source = std::fs::read_to_string(&resolved).map_err(|e| ScriptError::Script {
                path: path_key.clone(),
                message: format!("failed to read script: {e}"),
            })?;
            validate_lookup_literals(&source, &path_key, &scripting.data_sources)?;
            if hook == ScriptPhase::Response && script_needs_response_wire_meta(&source) {
                scripting.needs_response_wire_meta = true;
            }
            // Prefer the config-relative path for consumer errors when available.
            let consumer_path = action.value.clone();
            for site in scan_metric_sites(&source)? {
                scripting
                    .metrics
                    .register(&site.name, site.label_keys.clone())?;
                scripting.metric_consumers.record(
                    site.name.clone(),
                    MetricConsumerRef {
                        kind: ConsumerKind::Rhai,
                        path: consumer_path.clone(),
                        line: Some(site.line),
                        symbol: site.api.clone(),
                    },
                );
            }
            let engine = rhai::Engine::new();
            let ast = engine.compile(&source).map_err(|e| ScriptError::Script {
                path: path_key.clone(),
                message: e.to_string(),
            })?;
            let id = scripting.scripts.len();
            scripting.scripts.push(CompiledScript {
                path: path_key.clone(),
                rule_name: rule.name.clone(),
                hook,
                ast,
            });
            scripting
                .script_index
                .insert((rule.name.clone(), path_key), id);
            id
        };
        scripting.rules_scripts.push(ScriptRef {
            rule_name: rule.name.clone(),
            hook,
            path: action.value.clone(),
            script_id,
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use conduit_config::load_yaml;
    use std::fs;
    use std::path::PathBuf;

    fn fixtures_config_dir() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/config")
    }

    #[test]
    fn rhai_resolved_path_matches_config_relative_script_location() {
        let base = fixtures_config_dir();
        let yaml = include_str!("../../../tests/fixtures/config/with-rhai-minimal.yaml");
        let cfg = load_yaml(yaml).unwrap();
        let expected = base
            .join("../rhai/set-vip-pool.rhai")
            .canonicalize()
            .expect("fixture script path");
        let compiled = compile_from_config(&cfg, Some(&base)).unwrap();
        assert_eq!(compiled.scripts.len(), 1);
        let resolved = PathBuf::from(&compiled.scripts[0].path);
        assert_eq!(
            resolved.canonicalize().expect("resolved script path"),
            expected
        );
    }

    #[test]
    fn rhai_relative_path_ignored_by_cwd_when_base_dir_set() {
        let root = tempfile::TempDir::new().unwrap();
        let config_dir = root.path().join("cfg");
        let scripts_dir = config_dir.join("scripts");
        fs::create_dir_all(&scripts_dir).unwrap();
        fs::write(scripts_dir.join("hook.rhai"), "// ok\n").unwrap();

        let other = root.path().join("other");
        fs::create_dir_all(&other).unwrap();

        let cfg = load_yaml(
            r#"schema_version: 1
listeners:
  threads: 1
  listeners:
    - address: "127.0.0.1:15353"
      protocol: udp
pools:
  - name: default
    backends:
      - address: "127.0.0.1:5300"
        weight: 100
rules:
  match_mode: first_match
  rules:
    - name: hook
      hook: request
      selectors: []
      actions:
        - type: rhai
          value: scripts/hook.rhai
"#,
        )
        .unwrap();

        let original = std::env::current_dir().unwrap();
        std::env::set_current_dir(&other).unwrap();
        let result = compile_from_config(&cfg, Some(config_dir.as_path()));
        std::env::set_current_dir(original).unwrap();

        result.expect("config dir must resolve script even when cwd differs");
    }

    #[test]
    fn minimal_script_compiles_in_snapshot() {
        let yaml = include_str!("../../../tests/fixtures/config/with-rhai-minimal.yaml");
        let cfg = load_yaml(yaml).unwrap();
        let compiled = compile_from_config(&cfg, Some(&fixtures_config_dir())).unwrap();
        assert_eq!(compiled.scripts.len(), 1);
    }

    #[test]
    fn metric_registry_reconciles_on_reload() {
        let base = fixtures_config_dir();
        let yaml1 = include_str!("../../../tests/fixtures/config/with-rhai-block-hits.yaml");
        let cfg1 = load_yaml(yaml1).unwrap();
        let snap1 = compile_from_config(&cfg1, Some(&base)).unwrap();
        assert!(snap1.metrics.metrics.contains_key("block_hits"));

        let yaml2 = include_str!("../../../tests/fixtures/config/with-rhai-minimal.yaml");
        let cfg2 = load_yaml(yaml2).unwrap();
        let snap2 = compile_from_config(&cfg2, Some(&base)).unwrap();
        assert!(!snap2.metrics.metrics.contains_key("block_hits"));
    }

    #[test]
    fn slow_login_yaml_registers_user_metric() {
        let yaml = include_str!("../../../tests/fixtures/config/with-rhai-slow-login.yaml");
        let cfg = load_yaml(yaml).unwrap();
        let compiled = compile_from_config(&cfg, Some(&fixtures_config_dir())).unwrap();
        assert!(compiled.metrics.metrics.contains_key("slow_login"));
    }

    #[test]
    fn reject_unknown_lookup_literal_at_compile() {
        let base = fixtures_config_dir();
        let script_path = base.join("../rhai/bad-table-lookup.rhai");
        std::fs::write(
            &script_path,
            r#"lookup("not_a_table", txn.question().qname);"#,
        )
        .unwrap();
        let yaml = r#"schema_version: 1
listeners:
  threads: 1
  listeners:
    - address: "127.0.0.1:15353"
      protocol: udp
forward:
  outstanding_per_backend: 100
  timeout_ms: 2000
pools:
  - name: default
    backends:
      - address: "127.0.0.1:5300"
data_sources:
  - name: blocklist
    type: csv
    path: ../data/blocklist.csv
    key_column: qname
    value_column: action
rules:
  match_mode: first_match
  rules:
    - name: bad
      hook: request
      selectors: []
      actions:
        - type: rhai
          value: ../rhai/bad-table-lookup.rhai
"#
        .to_string();
        let cfg = load_yaml(&yaml).unwrap();
        let err = compile_from_config(&cfg, Some(&base)).unwrap_err();
        assert!(err
            .to_string()
            .contains("unknown data source 'not_a_table'"));
        let _ = std::fs::remove_file(script_path);
    }

    #[test]
    fn reject_unknown_user_metric_export_name_at_compile() {
        let base = fixtures_config_dir();
        let yaml = r#"
schema_version: 1
listeners:
  threads: 1
  listeners:
    - address: "127.0.0.1:15353"
      protocol: udp
forward:
  outstanding_per_backend: 100
  timeout_ms: 2000
pools:
  - name: default
    backends:
      - address: "127.0.0.1:5300"
metrics:
  enabled: true
  profile: full
  user_metrics:
    - name: not_registered
      export: minimal
rules:
  match_mode: first_match
  rules:
    - name: src
      hook: request
      selectors: []
      actions:
        - type: rhai
          value: ../rhai/set-vip-pool.rhai
"#;
        let cfg = load_yaml(yaml).unwrap();
        let err = compile_from_config(&cfg, Some(&base)).unwrap_err();
        assert!(err.to_string().contains("unknown user metric"));
    }

    #[test]
    fn enables_response_wire_meta_when_script_references_truncated() {
        let dir =
            std::env::temp_dir().join(format!("conduit-script-wire-meta-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let script_path = dir.join("check-tc.rhai");
        std::fs::write(
            &script_path,
            r#"if txn.response()?.truncated { txn.request_retry(); }"#,
        )
        .unwrap();
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
pools:
  - name: default
    backends:
      - address: "127.0.0.1:5300"
control:
  listen_address: "127.0.0.1:5199"
rules:
  match_mode: first_match
  rules:
    - name: tc-retry
      hook: response
      selectors:
        - type: qname
          value: "."
      actions:
        - type: rhai
          value: "{}"
"#,
            script_path.display()
        );
        let cfg = load_yaml(&yaml).unwrap();
        let compiled = compile_from_config(&cfg, Some(&dir)).unwrap();
        assert!(compiled.needs_response_wire_meta);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn response_wire_meta_disabled_for_rcode_only_script() {
        let base = fixtures_config_dir();
        let yaml = include_str!("../../../tests/fixtures/config/with-rhai-servfail-retry.yaml");
        let cfg = load_yaml(yaml).unwrap();
        let compiled = compile_from_config(&cfg, Some(&base)).unwrap();
        assert!(!compiled.needs_response_wire_meta);
    }
}
