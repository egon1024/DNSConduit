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
    if patch.metrics.is_some() {
        errors.push(
            "overlay patch must not include `metrics` — edit the config file and reload".into(),
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
    fn rejects_metrics_and_tracing_in_overlay_patch() {
        use conduit_proto::config::{MetricsConfig, TracingConfig};

        let metrics_patch = Config {
            schema_version: 1,
            metrics: Some(MetricsConfig {
                enabled: true,
                profile: "full".into(),
                prometheus: None,
                otel: None,
            }),
            ..Default::default()
        };
        assert!(!validate_overlay_patch(&metrics_patch).ok);

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
    }

    #[test]
    fn accepts_pool_only_overlay_patch() {
        let yaml = include_str!("../../../tests/manual/config/phase-5-overlay-pools-only.yaml");
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
