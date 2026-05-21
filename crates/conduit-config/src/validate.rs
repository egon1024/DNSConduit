use crate::backend::effective_backend_weight;
use crate::logging::validate_logging;
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
}
