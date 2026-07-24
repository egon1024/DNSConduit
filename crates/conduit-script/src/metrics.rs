use crate::error::ScriptError;
use conduit_metrics::{resolve_metrics_plan, BuiltinProfile, CompiledMetricsPlan};
use conduit_proto::config::MetricsConfig;
use std::collections::{HashMap, HashSet};

const DISALLOWED_LABEL_KEYS: &[&str] = &[
    "qname",
    "client",
    "client_ip",
    "client_addr",
    "backend",
    "txn_id",
    "dns_id",
    "address",
    "ip",
    "host",
    "query",
    "zone",
    "fqdn",
];

#[derive(Debug, Clone, Default)]
pub struct MetricRegistry {
    pub metrics: HashMap<String, UserMetricDef>,
}

/// When a Rhai user metric is recorded on the hot path / exported.
///
/// Prefer [`UserMetricDef::collect`] / [`UserMetricDef::emit`]. The legacy
/// `export_tier` field remains for call sites that still speak in
/// minimal/full terms; it is derived from collect/emit at apply time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum UserMetricExportTier {
    /// Record/export whenever metrics are enabled (legacy `export: minimal`).
    Minimal,
    /// Record/export only on standard-tier plans (legacy `export: full`; default).
    #[default]
    Full,
}

impl UserMetricExportTier {
    pub fn exports_at_profile(self, profile: BuiltinProfile) -> bool {
        match profile {
            BuiltinProfile::Off => false,
            BuiltinProfile::Full => true,
            BuiltinProfile::Minimal => self == UserMetricExportTier::Minimal,
        }
    }
}

#[derive(Debug, Clone)]
pub struct UserMetricDef {
    pub name: String,
    pub label_keys: HashSet<String>,
    /// Host/hot-path collect mask (MetricStore / UserRegistry increments).
    pub collect: bool,
    /// Prometheus/OTLP emit mask.
    pub emit: bool,
    /// Legacy tier mirror of collect/emit for older helpers.
    pub export_tier: UserMetricExportTier,
}

impl MetricRegistry {
    pub fn register(&mut self, name: &str, label_keys: HashSet<String>) -> Result<(), ScriptError> {
        for key in &label_keys {
            validate_label_key(key)?;
        }
        if self.metrics.contains_key(name) {
            let existing = self.metrics.get(name).unwrap();
            if existing.label_keys != label_keys {
                return Err(ScriptError::Metric {
                    name: name.into(),
                    message: "conflicting label keys across scripts".into(),
                });
            }
            return Ok(());
        }
        self.metrics.insert(
            name.to_string(),
            UserMetricDef {
                name: name.to_string(),
                label_keys,
                // Defaults match legacy `export: full` until `apply_user_metric_exports`
                // reconciles against the compiled plan.
                collect: true,
                emit: true,
                export_tier: UserMetricExportTier::Full,
            },
        );
        Ok(())
    }

    pub fn should_collect(&self, name: &str) -> bool {
        self.metrics.get(name).map(|d| d.collect).unwrap_or(false)
    }

    pub fn should_emit(&self, name: &str) -> bool {
        self.metrics.get(name).map(|d| d.emit).unwrap_or(false)
    }

    pub fn validate_runtime_labels(
        &self,
        name: &str,
        labels: &HashMap<String, String>,
    ) -> Result<(), ScriptError> {
        let def = self.metrics.get(name).ok_or_else(|| ScriptError::Metric {
            name: name.into(),
            message: "metric not registered at script load".into(),
        })?;
        for key in labels.keys() {
            validate_label_key(key)?;
            if !def.label_keys.contains(key) {
                return Err(ScriptError::Metric {
                    name: name.into(),
                    message: format!("unexpected label key '{key}'"),
                });
            }
        }
        Ok(())
    }

    /// Backward-compatible helper used by older call sites; prefer
    /// [`Self::should_collect`].
    pub fn exports_at_profile(&self, name: &str, _profile: BuiltinProfile) -> bool {
        self.should_collect(name)
    }

    /// Apply `metrics.user_metrics[]` and plan defaults to registered metrics.
    ///
    /// Validates that every config entry names a script-registered metric.
    /// Collect/emit come from [`resolve_metrics_plan`] (explicit keys or
    /// deprecated `export` alias).
    pub fn apply_user_metric_exports(
        &mut self,
        metrics: Option<&MetricsConfig>,
    ) -> Result<(), ScriptError> {
        let plan = match resolve_metrics_plan(metrics) {
            Ok(r) => r.plan,
            Err(errs) => {
                return Err(ScriptError::Metric {
                    name: String::new(),
                    message: errs.join("; "),
                });
            }
        };
        self.apply_from_plan(metrics, &plan)
    }

    pub fn apply_from_plan(
        &mut self,
        metrics: Option<&MetricsConfig>,
        plan: &CompiledMetricsPlan,
    ) -> Result<(), ScriptError> {
        if let Some(m) = metrics {
            let mut seen = HashSet::new();
            for entry in &m.user_metrics {
                if entry.name.is_empty() {
                    return Err(ScriptError::Metric {
                        name: String::new(),
                        message: "user_metrics entry name must not be empty".into(),
                    });
                }
                if !seen.insert(entry.name.clone()) {
                    return Err(ScriptError::Metric {
                        name: entry.name.clone(),
                        message: "duplicate user_metrics name".into(),
                    });
                }
                if !self.metrics.contains_key(&entry.name) {
                    return Err(ScriptError::Metric {
                        name: entry.name.clone(),
                        message: format!(
                            "unknown user metric '{}' — not registered by any Rhai script",
                            entry.name
                        ),
                    });
                }
            }
        }

        for (name, def) in self.metrics.iter_mut() {
            let collect = plan.user_collect_for(name);
            let emit = plan.user_emit_for(name);
            def.collect = collect;
            def.emit = emit;
            def.export_tier = if collect && emit && !plan.user_metrics_standard_tier() {
                UserMetricExportTier::Minimal
            } else if collect && emit {
                // Standard tier with collect+emit: treat as Full (also covers
                // explicit collect/emit true on standard).
                UserMetricExportTier::Full
            } else if collect && !emit {
                // Collect-only: no legacy tier; keep Full as inert label.
                UserMetricExportTier::Full
            } else {
                UserMetricExportTier::Full
            };
        }
        Ok(())
    }
}

/// Scan script source for `metrics.inc("name"` / `metric_inc("name"` and optional label map keys.
pub fn scan_metrics_from_source(
    source: &str,
) -> Result<Vec<(String, HashSet<String>)>, ScriptError> {
    let mut found = Vec::new();
    for line in source.lines() {
        if let Some(name) = extract_metric_name(line) {
            let labels = extract_label_keys(line);
            for key in &labels {
                validate_label_key(key)?;
            }
            found.push((name, labels));
        }
    }
    Ok(found)
}

fn extract_metric_name(line: &str) -> Option<String> {
    let needle = if line.contains("metrics.inc") {
        "metrics.inc"
    } else if line.contains("metric_inc") {
        "metric_inc"
    } else {
        return None;
    };
    let idx = line.find(needle)?;
    let rest = &line[idx..];
    let open = rest.find('(')?;
    let after = &rest[open + 1..];
    let quote = after.find('"')?;
    let after_quote = &after[quote + 1..];
    let end = after_quote.find('"')?;
    Some(after_quote[..end].to_string())
}

fn extract_label_keys(line: &str) -> HashSet<String> {
    let mut keys = HashSet::new();
    if let Some(hash_idx) = line.find("#{") {
        let fragment = &line[hash_idx + 2..];
        for part in fragment.split(',') {
            if let Some((key, _)) = part.split_once(':') {
                let key = key.trim();
                if !key.is_empty() {
                    keys.insert(key.to_string());
                }
            }
        }
    }
    keys
}

pub fn validate_label_key(key: &str) -> Result<(), ScriptError> {
    if DISALLOWED_LABEL_KEYS.contains(&key) {
        return Err(ScriptError::Metric {
            name: key.into(),
            message: "high-cardinality label key is not allowed".into(),
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reject_qname_label_at_load() {
        let err = validate_label_key("qname").unwrap_err();
        assert!(err.to_string().contains("cardinality"));
    }

    #[test]
    fn scan_metric_inc_from_source() {
        let src = r#"metric_inc("block_hits", 1, #{ category: "x" });"#;
        let metrics = scan_metrics_from_source(src).unwrap();
        assert_eq!(metrics[0].0, "block_hits");
        assert!(metrics[0].1.contains("category"));
    }

    #[test]
    fn export_tier_defaults_to_full() {
        let mut reg = MetricRegistry::default();
        reg.register("hits", HashSet::new()).unwrap();
        let mut cfg = conduit_proto::config::MetricsConfig {
            enabled: Some(true),
            profile: String::new(),
            prometheus: None,
            otel: None,
            user_metrics: vec![],
            base: "minimal".into(),
            categories: None,
            granularity: None,
            collection: Default::default(),
            event_export: None,
        };
        reg.apply_user_metric_exports(Some(&cfg)).unwrap();
        assert!(!reg.should_collect("hits"));
        assert!(!reg.should_emit("hits"));

        cfg.base = "standard".into();
        reg.apply_user_metric_exports(Some(&cfg)).unwrap();
        assert!(reg.should_collect("hits"));
        assert!(reg.should_emit("hits"));
    }

    #[test]
    fn apply_user_metric_export_override() {
        use conduit_proto::config::{MetricsConfig, UserMetricExportConfig};

        let mut reg = MetricRegistry::default();
        reg.register("block_hits", HashSet::from(["category".into()]))
            .unwrap();
        let cfg = MetricsConfig {
            enabled: Some(true),
            profile: String::new(),
            prometheus: None,
            otel: None,
            user_metrics: vec![UserMetricExportConfig {
                name: "block_hits".into(),
                export: "minimal".into(),
                collect: None,
                emit: None,
            }],
            base: "minimal".into(),
            categories: None,
            granularity: None,
            collection: Default::default(),
            event_export: None,
        };
        reg.apply_user_metric_exports(Some(&cfg)).unwrap();
        assert!(reg.should_collect("block_hits"));
        assert!(reg.should_emit("block_hits"));
    }

    #[test]
    fn apply_explicit_collect_only() {
        use conduit_proto::config::{MetricsConfig, UserMetricExportConfig};

        let mut reg = MetricRegistry::default();
        reg.register("block_hits", HashSet::new()).unwrap();
        let cfg = MetricsConfig {
            enabled: Some(true),
            profile: String::new(),
            prometheus: None,
            otel: None,
            user_metrics: vec![UserMetricExportConfig {
                name: "block_hits".into(),
                export: String::new(),
                collect: Some(true),
                emit: Some(false),
            }],
            base: "standard".into(),
            categories: None,
            granularity: None,
            collection: Default::default(),
            event_export: None,
        };
        reg.apply_user_metric_exports(Some(&cfg)).unwrap();
        assert!(reg.should_collect("block_hits"));
        assert!(!reg.should_emit("block_hits"));
    }

    #[test]
    fn apply_rejects_unknown_user_metric_name() {
        use conduit_proto::config::{MetricsConfig, UserMetricExportConfig};

        let mut reg = MetricRegistry::default();
        let cfg = MetricsConfig {
            enabled: Some(true),
            profile: "full".into(),
            prometheus: None,
            otel: None,
            user_metrics: vec![UserMetricExportConfig {
                name: "missing".into(),
                export: "minimal".into(),
                collect: None,
                emit: None,
            }],
            base: String::new(),
            categories: None,
            granularity: None,
            collection: Default::default(),
            event_export: None,
        };
        let err = reg.apply_user_metric_exports(Some(&cfg)).unwrap_err();
        assert!(err.to_string().contains("unknown user metric"));
    }
}
