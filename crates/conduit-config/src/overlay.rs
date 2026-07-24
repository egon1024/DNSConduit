//! Overlay patch validation and YAML load helpers.

use crate::validate::ValidationResult;
use conduit_proto::config::Config;

/// Fields that must not appear in an API overlay patch (file layer + reload only).
pub fn validate_overlay_patch(patch: &Config) -> ValidationResult {
    let mut errors = Vec::new();

    if patch.rules.is_some() {
        errors.push(
            "overlay patch must not include `rules` — edit the config file and reload".into(),
        );
    }
    if patch.tracing.is_some() {
        errors.push(
            "overlay patch must not include `tracing` — edit the config file and reload".into(),
        );
    }

    ValidationResult {
        ok: errors.is_empty(),
        errors,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::file::{load_overlay_patch, load_yaml};
    use conduit_proto::config::RulesConfig;

    #[test]
    fn rejects_rules_in_overlay_patch() {
        let patch = Config {
            schema_version: 1,
            rules: Some(RulesConfig {
                match_mode: "first_match".into(),
                rules: vec![],
            }),
            ..Default::default()
        };
        let v = validate_overlay_patch(&patch);
        assert!(!v.ok);
        assert!(v.errors.iter().any(|e| e.contains("rules")));
    }

    #[test]
    fn accepts_metrics_rejects_tracing_in_overlay_patch() {
        use conduit_proto::config::{MetricsConfig, TracingConfig};

        let metrics_patch = Config {
            schema_version: 1,
            metrics: Some(MetricsConfig {
                enabled: Some(true),
                profile: "full".into(),
                prometheus: None,
                otel: None,
                user_metrics: vec![],
                base: String::new(),
                categories: None,
                granularity: None,
                collection: Default::default(),
                event_export: None,
            }),
            ..Default::default()
        };
        assert!(
            validate_overlay_patch(&metrics_patch).ok,
            "metrics overlays are eligible"
        );

        let tracing_patch = Config {
            schema_version: 1,
            tracing: Some(TracingConfig {
                enabled: true,
                activation: None,
                output: None,
            }),
            ..Default::default()
        };
        assert!(!validate_overlay_patch(&tracing_patch).ok);
        assert!(validate_overlay_patch(&tracing_patch)
            .errors
            .iter()
            .any(|e| e.contains("tracing")));
    }

    #[test]
    fn metrics_only_overlay_patch_validates_ok() {
        let yaml = r#"
schema_version: 1
metrics:
  categories:
    exclude: [process]
"#;
        let patch = load_overlay_patch(yaml).expect("metrics-only overlay");
        assert!(patch.metrics.is_some());
        let cats = patch
            .metrics
            .as_ref()
            .unwrap()
            .categories
            .as_ref()
            .expect("categories");
        assert!(cats.exclude_set);
        assert_eq!(cats.exclude, vec!["process".to_string()]);
        assert!(!cats.include_set);
        let v = validate_overlay_patch(&patch);
        assert!(v.ok, "{:?}", v.errors);
    }

    #[test]
    fn accepts_pool_only_overlay_patch() {
        let yaml =
            include_str!("../../../tests/manual/config/control-plane-overlay-pools-only.yml");
        let patch = load_overlay_patch(yaml).expect("sparse pools overlay");
        let v = validate_overlay_patch(&patch);
        assert!(v.ok, "{:?}", v.errors);
        assert!(patch.listeners.is_none());
        assert!(patch.rules.is_none());
    }

    #[test]
    fn load_overlay_patch_rejects_rules_key() {
        let yaml = include_str!("../../../tests/fixtures/config/with-rules.yaml");
        let err = load_overlay_patch(yaml).unwrap_err();
        assert!(err.to_string().contains("rules"));
    }

    #[test]
    fn load_yaml_still_allows_rules_for_file_layer() {
        let yaml = include_str!("../../../tests/fixtures/config/with-rules.yaml");
        let cfg = load_yaml(yaml).expect("file load");
        assert!(cfg.rules.is_some());
    }
}
