use crate::error::ScriptError;
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

#[derive(Debug, Clone)]
pub struct UserMetricDef {
    pub name: String,
    pub label_keys: HashSet<String>,
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
            },
        );
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

pub fn validate_label_key(key: &str) -> Result<(), ScriptError> {
    if DISALLOWED_LABEL_KEYS.contains(&key) {
        return Err(ScriptError::Metric {
            name: key.into(),
            message: "high-cardinality label key is not allowed".into(),
        });
    }
    Ok(())
}

/// Scan script source for `metric_inc("name"` and optional label map keys.
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
    let idx = line.find("metric_inc")?;
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
}
