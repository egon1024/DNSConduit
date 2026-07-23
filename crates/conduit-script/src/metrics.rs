use crate::error::ScriptError;
use conduit_metrics::BuiltinProfile;
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

/// When a Rhai user metric is recorded on the hot path.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum UserMetricExportTier {
    /// Record on `minimal` and `full` built-in profiles.
    Minimal,
    /// Record on `full` only (default for script-discovered metrics).
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
                export_tier: UserMetricExportTier::Full,
            },
        );
        Ok(())
    }

    pub fn exports_at_profile(&self, name: &str, profile: BuiltinProfile) -> bool {
        self.metrics
            .get(name)
            .map(|d| d.export_tier.exports_at_profile(profile))
            .unwrap_or(false)
    }

    pub fn apply_user_metric_exports(
        &mut self,
        metrics: Option<&MetricsConfig>,
    ) -> Result<(), ScriptError> {
        let Some(m) = metrics else {
            return Ok(());
        };
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
            let tier = parse_user_metric_export_tier(&entry.export)?;
            if let Some(def) = self.metrics.get_mut(&entry.name) {
                def.export_tier = tier;
            } else {
                return Err(ScriptError::Metric {
                    name: entry.name.clone(),
                    message: format!(
                        "unknown user metric '{}' — not registered by any Rhai script",
                        entry.name
                    ),
                });
            }
        }
        Ok(())
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
}

fn parse_user_metric_export_tier(export: &str) -> Result<UserMetricExportTier, ScriptError> {
    match export {
        "" | "full" => Ok(UserMetricExportTier::Full),
        "minimal" => Ok(UserMetricExportTier::Minimal),
        other => Err(ScriptError::Metric {
            name: other.into(),
            message: "user_metrics export must be 'minimal' or 'full'".into(),
        }),
    }
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
        assert!(!reg.exports_at_profile("hits", BuiltinProfile::Minimal));
        assert!(reg.exports_at_profile("hits", BuiltinProfile::Full));
    }

    #[test]
    fn apply_user_metric_export_override() {
        use conduit_proto::config::{MetricsConfig, UserMetricExportConfig};

        let mut reg = MetricRegistry::default();
        reg.register("block_hits", HashSet::from(["category".into()]))
            .unwrap();
        let cfg = MetricsConfig {
            enabled: true,
            profile: "minimal".into(),
            prometheus: None,
            otel: None,
            user_metrics: vec![UserMetricExportConfig {
                name: "block_hits".into(),
                export: "minimal".into(),
            }],
            base: String::new(),
            categories: None,
            granularity: None,
            collection: Default::default(),
            event_export: None,
        };
        reg.apply_user_metric_exports(Some(&cfg)).unwrap();
        assert!(reg.exports_at_profile("block_hits", BuiltinProfile::Minimal));
    }

    #[test]
    fn apply_rejects_unknown_user_metric_name() {
        use conduit_proto::config::{MetricsConfig, UserMetricExportConfig};

        let mut reg = MetricRegistry::default();
        let cfg = MetricsConfig {
            enabled: true,
            profile: "full".into(),
            prometheus: None,
            otel: None,
            user_metrics: vec![UserMetricExportConfig {
                name: "missing".into(),
                export: "minimal".into(),
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
