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

/// Scan script source for `metrics.inc` / `metrics.inc_labels` / `metric_inc`
/// (and future read APIs) with line numbers for the consumer dependency graph.
pub fn scan_metric_sites(source: &str) -> Result<Vec<MetricScanSite>, ScriptError> {
    let mut found = Vec::new();
    for (line_idx, line) in source.lines().enumerate() {
        let line_no = (line_idx + 1) as u32;
        for site in scan_line_sites(line, line_no)? {
            found.push(site);
        }
    }
    Ok(found)
}

/// Scan script source for metric names and optional label map keys.
///
/// Prefer [`scan_metric_sites`] when line numbers / API kind are needed.
pub fn scan_metrics_from_source(
    source: &str,
) -> Result<Vec<(String, HashSet<String>)>, ScriptError> {
    Ok(scan_metric_sites(source)?
        .into_iter()
        .map(|s| (s.name, s.label_keys))
        .collect())
}

/// One static reference site in a Rhai source file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MetricScanSite {
    pub name: String,
    pub label_keys: HashSet<String>,
    /// 1-based line number.
    pub line: u32,
    /// Call form as written (`metrics.inc`, `metrics.inc_labels`, …).
    pub api: String,
}

/// Future read APIs (metric windows, etc.) — empty until those helpers ship.
const FUTURE_READ_APIS: &[&str] = &[];

fn scan_line_sites(line: &str, line_no: u32) -> Result<Vec<MetricScanSite>, ScriptError> {
    let mut sites = Vec::new();

    // Check longer / more specific names first so `metrics.inc` does not match
    // inside `metrics.inc_labels`.
    for (fn_name, api) in [
        ("metrics.inc_labels", "metrics.inc_labels"),
        ("metrics.inc", "metrics.inc"),
        ("metric_inc", "metric_inc"),
    ] {
        for rest in find_calls(line, fn_name) {
            if let Some(name) = extract_call_string_arg(rest) {
                let labels = extract_label_keys(line);
                for key in &labels {
                    validate_label_key(key)?;
                }
                sites.push(MetricScanSite {
                    name,
                    label_keys: labels,
                    line: line_no,
                    api: api.into(),
                });
            }
        }
    }

    for api in FUTURE_READ_APIS {
        for rest in find_calls(line, api) {
            if let Some(name) = extract_call_string_arg(rest) {
                sites.push(MetricScanSite {
                    name,
                    label_keys: HashSet::new(),
                    line: line_no,
                    api: (*api).into(),
                });
            }
        }
    }

    Ok(sites)
}

/// Word-boundary call finder (same approach as `lookup_scan`).
fn find_calls<'a>(line: &'a str, fn_name: &str) -> Vec<&'a str> {
    let bytes = line.as_bytes();
    let mut out = Vec::new();
    let mut start = 0;
    while let Some(rel) = line[start..].find(fn_name) {
        let idx = start + rel;
        let after = idx + fn_name.len();
        let before_ok = idx == 0 || !is_ident_byte(bytes[idx - 1]);
        let after_ok = bytes.get(after).map(|b| !is_ident_byte(*b)).unwrap_or(true);
        if before_ok && after_ok {
            out.push(&line[after..]);
        }
        start = after;
    }
    out
}

fn is_ident_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

fn extract_call_string_arg(rest: &str) -> Option<String> {
    let after_open = rest.trim_start().strip_prefix('(')?;
    let after_paren = after_open.trim_start();
    let quote = after_paren.chars().next()?;
    if quote != '"' && quote != '\'' {
        return None;
    }
    let after_quote = &after_paren[quote.len_utf8()..];
    let end = after_quote.find(quote)?;
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
    fn scan_metric_sites_inc_labels_with_line() {
        let src = "let x = 1;\nmetrics.inc_labels(\"block_hits\", 1, #{ category: \"eu\" });\n";
        let sites = scan_metric_sites(src).unwrap();
        assert_eq!(sites.len(), 1);
        assert_eq!(sites[0].name, "block_hits");
        assert_eq!(sites[0].line, 2);
        assert_eq!(sites[0].api, "metrics.inc_labels");
        assert!(sites[0].label_keys.contains("category"));
    }

    #[test]
    fn scan_inc_does_not_double_match_inc_labels() {
        let src = r#"metrics.inc_labels("hits", 1, #{ category: "x" });"#;
        let sites = scan_metric_sites(src).unwrap();
        assert_eq!(sites.len(), 1);
        assert_eq!(sites[0].api, "metrics.inc_labels");
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
