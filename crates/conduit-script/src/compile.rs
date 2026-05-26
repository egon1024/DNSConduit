use crate::data_sources::{load_data_sources, DataSourceStore};
use crate::error::ScriptError;
use crate::host::ScriptPhase;
use crate::metrics::{scan_metrics_from_source, MetricRegistry};
use conduit_proto::config::{Config, RhaiConfig, Rule};
use rhai::AST;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
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
    pub rule_id: String,
    pub hook: ScriptPhase,
    pub path: String,
    pub script_id: usize,
}

#[derive(Debug)]
pub struct CompiledScript {
    pub path: String,
    pub rule_id: String,
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
    pub rules_scripts: Vec<ScriptRef>,
}

impl CompiledScripting {
    pub fn script_ids_for_rule(&self, rule_id: &str, hook: ScriptPhase) -> Vec<usize> {
        self.rules_scripts
            .iter()
            .filter(|r| r.rule_id == rule_id && r.hook == hook)
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
    let data_sources = Arc::new(load_data_sources(&config.data_sources, base_dir)?);
    let snapshot_generation = SNAPSHOT_GENERATION.fetch_add(1, Ordering::Relaxed) + 1;

    let mut scripting = CompiledScripting {
        scripts: Vec::new(),
        script_index: HashMap::new(),
        data_sources,
        snapshot_generation,
        limits,
        metrics: MetricRegistry::default(),
        rules_scripts: Vec::new(),
    };

    let Some(rules) = config.rules.as_ref() else {
        return Ok(scripting);
    };

    for rule in &rules.rules {
        compile_rule_scripts(rule, base_dir, &mut scripting)?;
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
                rule_id: rule.id.clone(),
                message: "rhai action requires script path in value".into(),
            });
        }
        let resolved = resolve_path(base_dir, &action.value);
        let path_key = resolved.display().to_string();
        let script_id = if let Some(&id) = scripting
            .script_index
            .get(&(rule.id.clone(), path_key.clone()))
        {
            id
        } else {
            let source = std::fs::read_to_string(&resolved).map_err(|e| ScriptError::Script {
                path: path_key.clone(),
                message: format!("failed to read script: {e}"),
            })?;
            for (name, labels) in scan_metrics_from_source(&source)? {
                scripting.metrics.register(&name, labels)?;
            }
            let engine = rhai::Engine::new();
            let ast = engine.compile(&source).map_err(|e| ScriptError::Script {
                path: path_key.clone(),
                message: e.to_string(),
            })?;
            let id = scripting.scripts.len();
            scripting.scripts.push(CompiledScript {
                path: path_key.clone(),
                rule_id: rule.id.clone(),
                hook,
                ast,
            });
            scripting
                .script_index
                .insert((rule.id.clone(), path_key), id);
            id
        };
        scripting.rules_scripts.push(ScriptRef {
            rule_id: rule.id.clone(),
            hook,
            path: action.value.clone(),
            script_id,
        });
    }
    Ok(())
}

fn resolve_path(base_dir: Option<&Path>, path: &str) -> PathBuf {
    let p = Path::new(path);
    if p.is_absolute() {
        p.to_path_buf()
    } else if let Some(base) = base_dir {
        base.join(p)
    } else {
        p.to_path_buf()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use conduit_config::load_yaml;
    use std::path::PathBuf;

    fn fixtures_config_dir() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/config")
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
}
