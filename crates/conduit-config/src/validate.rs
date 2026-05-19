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
    fn accept_minimal_config() {
        let yaml = include_str!("../../../tests/fixtures/config/minimal.yaml");
        let cfg = load_yaml(yaml).unwrap();
        let result = validate(&cfg);
        assert!(result.ok, "errors: {:?}", result.errors);
    }
}
