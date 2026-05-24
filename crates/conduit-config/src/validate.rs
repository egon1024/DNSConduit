use crate::backend::effective_backend_weight;
use crate::logging::validate_logging;
use conduit_observation::{parse_extra_fields, parse_extra_tags};
use conduit_proto::config::Config;

#[derive(Debug, Clone)]
pub struct ValidationResult {
    pub ok: bool,
    pub errors: Vec<String>,
}

pub fn validate(cfg: &Config) -> ValidationResult {
    let mut errors = Vec::new();

    if cfg.schema_version != 1 {
        errors.push(format!("unsupported schema_version {}", cfg.schema_version));
    }

    if let Some(l) = &cfg.listeners {
        if l.threads == 0 {
            errors.push("listeners.threads must be >= 1".into());
        }
        for ln in &l.listeners {
            if ln.address.is_empty() {
                errors.push("listener address must not be empty".into());
            }
        }
    }

    if let Some(o) = &cfg.orchestrator {
        if o.max_attempts == 0 {
            errors.push("orchestrator.max_attempts must be >= 1".into());
        }
    }

    for p in &cfg.pools {
        if p.name.is_empty() {
            errors.push("pool name must not be empty".into());
        }
        if p.backends.is_empty() {
            errors.push(format!("pool '{}' has no backends", p.name));
        }
        for backend in &p.backends {
            if effective_backend_weight(backend) == 0 {
                errors.push(format!(
                    "pool '{}' backend '{}' weight must be >= 1",
                    p.name, backend.address
                ));
            }
        }
    }

    if let Err(e) = validate_logging(cfg.logging.as_ref()) {
        errors.push(e.to_string());
    }

    if let Some(obs) = &cfg.observation {
        if obs.queue_depth == 0 {
            errors.push(
                "observation.queue_depth must be >= 1 when observation section is present".into(),
            );
        }
        if !matches!(obs.drop_policy.as_str(), "drop_oldest" | "drop_newest") {
            errors.push(format!(
                "observation.drop_policy '{}' must be drop_oldest or drop_newest",
                obs.drop_policy
            ));
        }
        for (i, sink) in obs.sinks.iter().enumerate() {
            if sink.r#type != "dnstap" {
                errors.push(format!(
                    "observation.sinks[{}].type '{}' is not supported (phase 2: dnstap only)",
                    i, sink.r#type
                ));
            }
            if sink.export_id.is_empty() {
                errors.push(format!(
                    "observation.sinks[{}].export_id must not be empty",
                    i
                ));
            }
            if sink.destinations.is_empty() {
                errors.push(format!(
                    "observation.sinks[{}].destinations must not be empty",
                    i
                ));
            }
            for dest in &sink.destinations {
                if !dest.starts_with("unix:") && !dest.starts_with("tcp:") {
                    errors.push(format!(
                        "observation.sinks[{}] destination '{}' must start with unix: or tcp:",
                        i, dest
                    ));
                }
            }
            for e in &sink.emit {
                if e != "query" && e != "response" && e != "retry" {
                    errors.push(format!(
                        "observation.sinks[{}].emit '{}' must be query, response, or retry",
                        i, e
                    ));
                }
            }
            if let Err(e) = parse_extra_fields(&sink.extra_fields) {
                errors.push(format!("observation.sinks[{i}]: {e}"));
            }
            let has_tags = sink.extra_fields.iter().any(|f| f == "tags");
            if let Err(e) = parse_extra_tags(&sink.extra_tags, has_tags) {
                errors.push(format!("observation.sinks[{i}]: {e}"));
            }
        }
    }

    if let Some(rules) = &cfg.rules {
        if rules.match_mode != "first_match" {
            errors.push(format!(
                "unsupported rules.match_mode '{}', only first_match",
                rules.match_mode
            ));
        }
        for rule in &rules.rules {
            if rule.id.is_empty() {
                errors.push("rule id must not be empty".into());
            }
            if rule.hook != "request" && rule.hook != "response" {
                errors.push(format!(
                    "rule '{}' has invalid hook '{}'",
                    rule.id, rule.hook
                ));
            }
            for sel in &rule.selectors {
                if !matches!(
                    sel.r#type.as_str(),
                    "qname_suffix" | "qname_exact" | "qtype" | "rcode" | "tag"
                ) {
                    errors.push(format!(
                        "rule '{}' has unknown selector type '{}'",
                        rule.id, sel.r#type
                    ));
                }
            }
            for act in &rule.actions {
                if !matches!(
                    act.r#type.as_str(),
                    "set_pool" | "set_tag" | "retry_pool" | "drop" | "set_rcode"
                ) {
                    errors.push(format!(
                        "rule '{}' has unknown action type '{}'",
                        rule.id, act.r#type
                    ));
                }
            }
        }
    }

    ValidationResult {
        ok: errors.is_empty(),
        errors,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::file::load_yaml;

    #[test]
    fn reject_zero_listener_threads() {
        let yaml = include_str!("../../../tests/fixtures/config/invalid_listener_threads.yaml");
        let cfg = load_yaml(yaml).unwrap();
        let result = validate(&cfg);
        assert!(!result.ok);
        assert!(result.errors.iter().any(|e| e.contains("threads")));
    }

    #[test]
    fn reject_zero_backend_weight() {
        let yaml = include_str!("../../../tests/fixtures/config/minimal.yaml");
        let mut cfg = load_yaml(yaml).unwrap();
        cfg.pools[0].backends[0].weight = Some(0);
        let result = validate(&cfg);
        assert!(!result.ok);
        assert!(result.errors.iter().any(|e| e.contains("weight")));
    }

    #[test]
    fn accept_backend_without_weight_field() {
        let yaml = include_str!("../../../tests/fixtures/config/minimal-no-weight.yaml");
        let cfg = load_yaml(yaml).unwrap();
        assert!(validate(&cfg).ok);
    }

    #[test]
    fn accept_with_rules_config() {
        let yaml = include_str!("../../../tests/fixtures/config/with-rules.yaml");
        let cfg = load_yaml(yaml).unwrap();
        let result = validate(&cfg);
        assert!(result.ok, "errors: {:?}", result.errors);
    }

    #[test]
    fn accept_minimal_config() {
        let yaml = include_str!("../../../tests/fixtures/config/minimal.yaml");
        let cfg = load_yaml(yaml).unwrap();
        let result = validate(&cfg);
        assert!(result.ok, "errors: {:?}", result.errors);
    }

    #[test]
    fn accept_with_dnstap_config() {
        let yaml = include_str!("../../../tests/fixtures/config/with-dnstap.yaml");
        let cfg = load_yaml(yaml).unwrap();
        assert!(validate(&cfg).ok);
    }

    #[test]
    fn accept_no_sinks_observation() {
        let yaml = include_str!("../../../tests/fixtures/config/no-sinks.yaml");
        let cfg = load_yaml(yaml).unwrap();
        assert!(validate(&cfg).ok);
    }

    #[test]
    fn accept_with_dnstap_extra_config() {
        let yaml = include_str!("../../../tests/fixtures/config/with-dnstap-extra.yaml");
        let cfg = load_yaml(yaml).unwrap();
        assert!(validate(&cfg).ok);
        let snap = conduit_observation::compile_from_config(&cfg);
        assert!(snap.enabled);
        assert!(snap.sinks[0].extra_fields.len() >= 3);
    }

    #[test]
    fn reject_unknown_extra_field() {
        let yaml = include_str!("../../../tests/fixtures/config/with-dnstap.yaml");
        let mut cfg = load_yaml(yaml).unwrap();
        cfg.observation.as_mut().unwrap().sinks[0]
            .extra_fields
            .push("upstream_pool".into());
        let result = validate(&cfg);
        assert!(!result.ok);
        assert!(result.errors.iter().any(|e| e.contains("extra_fields")));
    }

    #[test]
    fn reject_extra_tags_without_tags_field() {
        let yaml = include_str!("../../../tests/fixtures/config/with-dnstap.yaml");
        let mut cfg = load_yaml(yaml).unwrap();
        let sink = &mut cfg.observation.as_mut().unwrap().sinks[0];
        sink.extra_tags = vec!["vip".into()];
        let result = validate(&cfg);
        assert!(!result.ok);
        assert!(result.errors.iter().any(|e| e.contains("extra_tags")));
    }

    #[test]
    fn reject_invalid_sink_type_and_emit() {
        let yaml = include_str!("../../../tests/fixtures/config/minimal.yaml");
        let mut cfg = load_yaml(yaml).unwrap();
        cfg.observation
            .as_mut()
            .unwrap()
            .sinks
            .push(conduit_proto::config::ObservationSink {
                r#type: "syslog".into(),
                export_id: "x".into(),
                destinations: vec!["unix:/tmp/x".into()],
                emit: vec!["bogus".into()],
                filters: None,
                extra_fields: vec![],
                extra_tags: vec![],
            });
        let result = validate(&cfg);
        assert!(!result.ok);
        assert!(result.errors.iter().any(|e| e.contains("dnstap")));
        assert!(result.errors.iter().any(|e| e.contains("emit")));
    }
}
