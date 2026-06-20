use crate::backend::effective_backend_weight;
use crate::forward::{
    parse_sources_v4, parse_sources_v6, parse_upstream_transport,
    validate_upstream_backend_addresses,
};
use crate::logging::validate_logging;
use conduit_events::{
    parse_connect_retry, parse_extra_fields, parse_extra_tags, validate_sink_identity_uniqueness,
};
use conduit_proto::config::Config;

fn configured_source_v4_addrs(cfg: &Config) -> std::collections::HashSet<std::net::Ipv4Addr> {
    let mut addrs = std::collections::HashSet::new();
    if let Some(f) = &cfg.forward {
        if let Ok(v) = parse_sources_v4(&f.sources_v4) {
            addrs.extend(v);
        }
    }
    for p in &cfg.pools {
        if let Ok(v) = parse_sources_v4(&p.sources_v4) {
            addrs.extend(v);
        }
    }
    addrs
}

fn configured_source_v6_addrs(cfg: &Config) -> std::collections::HashSet<std::net::Ipv6Addr> {
    let mut addrs = std::collections::HashSet::new();
    if let Some(f) = &cfg.forward {
        if let Ok(v) = parse_sources_v6(&f.sources_v6) {
            addrs.extend(v);
        }
    }
    for p in &cfg.pools {
        if let Ok(v) = parse_sources_v6(&p.sources_v6) {
            addrs.extend(v);
        }
    }
    addrs
}

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

    if let Some(f) = &cfg.forward {
        if !f.source_selection.is_empty() && f.source_selection != "round_robin" {
            errors.push(format!(
                "forward.source_selection '{}' must be round_robin (slice A)",
                f.source_selection
            ));
        }
        if let Err(e) = parse_sources_v4(&f.sources_v4) {
            errors.push(format!("forward.{e}"));
        }
        if let Err(e) = parse_sources_v6(&f.sources_v6) {
            errors.push(format!("forward.{e}"));
        }
        if let Err(e) = parse_upstream_transport(&f.upstream_transport) {
            errors.push(e);
        }
    }

    errors.extend(validate_upstream_backend_addresses(cfg));

    let mut pool_names = std::collections::HashSet::new();
    for p in &cfg.pools {
        if p.name.is_empty() {
            errors.push("pool name must not be empty".into());
        } else if !pool_names.insert(p.name.clone()) {
            errors.push(format!("duplicate pool name '{}'", p.name));
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
        if let Err(e) = parse_sources_v4(&p.sources_v4) {
            errors.push(format!("pool '{}': {e}", p.name));
        }
        if let Err(e) = parse_sources_v6(&p.sources_v6) {
            errors.push(format!("pool '{}': {e}", p.name));
        }
    }

    if let Err(e) = validate_logging(cfg.logging.as_ref()) {
        errors.push(e.to_string());
    }

    if let Some(obs) = &cfg.events {
        if obs.queue_depth == 0 {
            errors.push("events.queue_depth must be >= 1 when events section is present".into());
        }
        if !matches!(obs.drop_policy.as_str(), "drop_oldest" | "drop_newest") {
            errors.push(format!(
                "events.drop_policy '{}' must be drop_oldest or drop_newest",
                obs.drop_policy
            ));
        }
        for (i, sink) in obs.sinks.iter().enumerate() {
            if sink.r#type != "dnstap" {
                errors.push(format!(
                    "events.sinks[{}].type '{}' is not supported (phase 2: dnstap only)",
                    i, sink.r#type
                ));
            }

            if let Err(e) = conduit_events::resolve_sink_identity(sink) {
                errors.push(format!("events.sinks[{i}]: {e}"));
            }
            if sink.destinations.is_empty() {
                errors.push(format!(
                    "events.sinks[{}].destinations must not be empty",
                    i
                ));
            }
            for dest in &sink.destinations {
                if !dest.starts_with("unix:") && !dest.starts_with("tcp:") {
                    errors.push(format!(
                        "events.sinks[{}] destination '{}' must start with unix: or tcp:",
                        i, dest
                    ));
                }
            }
            for e in &sink.emit {
                if e != "query" && e != "response" && e != "retry" {
                    errors.push(format!(
                        "events.sinks[{}].emit '{}' must be query, response, or retry",
                        i, e
                    ));
                }
            }
            if let Err(e) = parse_extra_fields(&sink.extra_fields) {
                errors.push(format!("events.sinks[{i}]: {e}"));
            }
            let has_tags = sink.extra_fields.iter().any(|f| f == "tags");
            if let Err(e) = parse_extra_tags(&sink.extra_tags, has_tags) {
                errors.push(format!("events.sinks[{i}]: {e}"));
            }
            if let Some(ref name) = sink.name {
                if name.is_empty() {
                    errors.push(format!("events.sinks[{i}].name must not be empty"));
                }
            }
            if let Err(e) = parse_connect_retry(sink) {
                errors.push(format!("events.sinks[{i}].connect_retry: {e}"));
            }
            if let Some(ref filters) = sink.filters {
                for (j, sel) in filters.selectors.iter().enumerate() {
                    if let Err(e) =
                        conduit_events::validate_non_rule_selector_type(sel.r#type.as_str())
                    {
                        errors.push(format!("events.sinks[{i}].filters.selectors[{j}]: {e}"));
                    }
                    if sel.r#type == "sample_percent"
                        && conduit_events::parse_selector_sample_percent(&sel.value).is_err()
                    {
                        errors.push(format!(
                            "events.sinks[{i}].filters.selectors[{j}] sample_percent must be in [0, 100]"
                        ));
                    }
                    if let Err(e) =
                        conduit_events::validate_selector_sample_key_fields(sel, false, true)
                    {
                        errors.push(format!("events.sinks[{i}].filters.selectors[{j}]: {e}"));
                    }
                }
                if let Some(percent) = filters.sample_percent {
                    if let Err(e) = conduit_events::parse_sample_percent(Some(percent)) {
                        errors.push(format!("events.sinks[{i}].filters: {e}"));
                    }
                }
                if let Err(e) = conduit_events::validate_top_level_sample_key_fields(
                    filters.sample_key.as_deref(),
                    filters.sample_key_from.as_deref(),
                    true,
                ) {
                    errors.push(format!("events.sinks[{i}].filters: {e}"));
                }
                if filters.pool.as_ref().is_some_and(|p| p.is_empty()) {
                    errors.push(format!("events.sinks[{i}].filters.pool must not be empty"));
                }
                if filters.backend.as_ref().is_some_and(|b| b.is_empty()) {
                    errors.push(format!(
                        "events.sinks[{i}].filters.backend must not be empty"
                    ));
                }
            }
        }
        errors.extend(validate_sink_identity_uniqueness(&obs.sinks));
    }

    if let Some(rules) = &cfg.rules {
        if rules.match_mode != "first_match" {
            errors.push(format!(
                "unsupported rules.match_mode '{}', only first_match",
                rules.match_mode
            ));
        }
        let allowed_v4 = configured_source_v4_addrs(cfg);
        let allowed_v6 = configured_source_v6_addrs(cfg);
        let mut rule_names = std::collections::HashSet::new();
        for rule in &rules.rules {
            if rule.name.is_empty() {
                errors.push("rule name must not be empty".into());
            } else if !rule_names.insert(rule.name.clone()) {
                errors.push(format!("duplicate rule name '{}'", rule.name));
            }
            if rule.hook != "request" && rule.hook != "response" {
                errors.push(format!(
                    "rule '{}' has invalid hook '{}'",
                    rule.name, rule.hook
                ));
            }
            for sel in &rule.selectors {
                if !matches!(
                    sel.r#type.as_str(),
                    "qname_suffix"
                        | "qname_exact"
                        | "qtype"
                        | "rcode"
                        | "tag"
                        | "sample_percent"
                        | "every_nth_worker"
                        | "every_nth_global"
                ) {
                    errors.push(format!(
                        "rule '{}' has unknown selector type '{}'",
                        rule.name, sel.r#type
                    ));
                }
                if sel.r#type == "sample_percent"
                    && conduit_events::parse_selector_sample_percent(&sel.value).is_err()
                {
                    errors.push(format!(
                        "rule '{}' selector sample_percent must be a float in [0, 100]",
                        rule.name
                    ));
                }
                if let Err(e) =
                    conduit_events::validate_selector_sample_key_fields(sel, true, false)
                {
                    errors.push(format!("rule '{}': {e}", rule.name));
                }
                if matches!(sel.r#type.as_str(), "every_nth_worker" | "every_nth_global")
                    && conduit_events::parse_every_nth(&sel.value).is_err()
                {
                    errors.push(format!(
                        "rule '{}' selector '{}' requires integer value >= 1",
                        rule.name, sel.r#type
                    ));
                }
            }
            for act in &rule.actions {
                if !matches!(
                    act.r#type.as_str(),
                    "set_pool"
                        | "set_tag"
                        | "retry"
                        | "retry_now"
                        | "set_retry_pool"
                        | "drop"
                        | "drop_now"
                        | "clear_drop"
                        | "clear_retry"
                        | "clear_retry_pool"
                        | "clear_tag"
                        | "set_rcode"
                        | "rhai"
                        | "set_source_v4"
                        | "set_source_v6"
                        | "set_retry_source_v4"
                        | "set_retry_source_v6"
                        | "clear_retry_source_v4"
                        | "clear_retry_source_v6"
                ) {
                    errors.push(format!(
                        "rule '{}' has unknown action type '{}'",
                        rule.name, act.r#type
                    ));
                }
                if act.r#type == "rhai" && act.value.is_empty() {
                    errors.push(format!(
                        "rule '{}' rhai action requires script path in value",
                        rule.name
                    ));
                }
                if matches!(
                    act.r#type.as_str(),
                    "retry" | "retry_now" | "clear_retry" | "set_rcode"
                ) && rule.hook != "response"
                {
                    errors.push(format!(
                        "rule '{}' action '{}' is only valid on response hook",
                        rule.name, act.r#type
                    ));
                }
                if act.r#type == "set_retry_pool" && act.value.is_empty() {
                    errors.push(format!(
                        "rule '{}' set_retry_pool requires a pool name in value",
                        rule.name
                    ));
                }
                if act.r#type == "clear_tag" && act.value.is_empty() {
                    errors.push(format!(
                        "rule '{}' clear_tag requires a non-empty tag key in value",
                        rule.name
                    ));
                }
                if matches!(act.r#type.as_str(), "set_source_v4" | "set_source_v6") {
                    if rule.hook != "request" {
                        errors.push(format!(
                            "rule '{}' action '{}' is only valid on request hook",
                            rule.name, act.r#type
                        ));
                    }
                    if act.value.is_empty() {
                        errors.push(format!(
                            "rule '{}' action '{}' requires an address in value",
                            rule.name, act.r#type
                        ));
                        continue;
                    }
                }
                if matches!(
                    act.r#type.as_str(),
                    "set_retry_source_v4" | "set_retry_source_v6"
                ) && act.value.is_empty()
                {
                    errors.push(format!(
                        "rule '{}' action '{}' requires an address in value",
                        rule.name, act.r#type
                    ));
                    continue;
                }
                if matches!(act.r#type.as_str(), "set_source_v4" | "set_retry_source_v4") {
                    match act.value.parse::<std::net::Ipv4Addr>() {
                        Ok(addr) => {
                            if allowed_v4.is_empty() {
                                errors.push(format!(
                                    "rule '{}' {} requires forward.sources_v4 or pool sources_v4",
                                    rule.name, act.r#type
                                ));
                            } else if !allowed_v4.contains(&addr) {
                                errors.push(format!(
                                    "rule '{}' {} '{}' is not in configured sources_v4",
                                    rule.name, act.r#type, act.value
                                ));
                            }
                        }
                        Err(_) => errors.push(format!(
                            "rule '{}' {} '{}' is not a valid IPv4 address",
                            rule.name, act.r#type, act.value
                        )),
                    }
                }
                if act.r#type == "set_source_v6" || act.r#type == "set_retry_source_v6" {
                    match act.value.parse::<std::net::Ipv6Addr>() {
                        Ok(addr) => {
                            if allowed_v6.is_empty() {
                                errors.push(format!(
                                    "rule '{}' {} requires forward.sources_v6 or pool sources_v6",
                                    rule.name, act.r#type
                                ));
                            } else if !allowed_v6.contains(&addr) {
                                errors.push(format!(
                                    "rule '{}' {} '{}' is not in configured sources_v6",
                                    rule.name, act.r#type, act.value
                                ));
                            }
                        }
                        Err(_) => errors.push(format!(
                            "rule '{}' {} '{}' is not a valid IPv6 address",
                            rule.name, act.r#type, act.value
                        )),
                    }
                }
            }
        }
    }

    let mut data_source_names = std::collections::HashSet::new();
    for ds in &cfg.data_sources {
        if ds.name.is_empty() {
            errors.push("data_sources entry name must not be empty".into());
        }
        if !data_source_names.insert(ds.name.clone()) {
            errors.push(format!("duplicate data_sources name '{}'", ds.name));
        }
        if ds.r#type != "csv" {
            errors.push(format!(
                "data_sources '{}' has unsupported type '{}', only csv is supported",
                ds.name, ds.r#type
            ));
        }
        if ds.path.is_empty() {
            errors.push(format!("data_sources '{}' path must not be empty", ds.name));
        }
    }

    if let Some(rhai) = &cfg.rhai {
        if rhai.max_operations == 0 {
            errors.push("rhai.max_operations must be >= 1 when set".into());
        }
        if rhai.max_call_depth == 0 {
            errors.push("rhai.max_call_depth must be >= 1 when set".into());
        }
    }

    if let Some(control) = &cfg.control {
        if control.listen_address.is_empty() {
            errors.push("control.listen_address must not be empty".into());
        } else if control
            .listen_address
            .parse::<std::net::SocketAddr>()
            .is_err()
        {
            errors.push(format!(
                "control.listen_address '{}' is not a valid socket address",
                control.listen_address
            ));
        }
    }

    errors.extend(conduit_metrics::validate_metrics_tracing(cfg));

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
    fn accept_no_sinks_events() {
        let yaml = include_str!("../../../tests/fixtures/config/no-sinks.yaml");
        let cfg = load_yaml(yaml).unwrap();
        assert!(validate(&cfg).ok);
    }

    #[test]
    fn reject_legacy_observation_top_level_key() {
        let yaml = r#"
schema_version: 1
listeners:
  threads: 1
  reuse_port: false
  listeners:
    - address: "127.0.0.1:15353"
      protocol: udp
forward:
  outstanding_per_backend: 100
  timeout_ms: 2000
orchestrator:
  max_attempts: 3
  max_txn_duration_ms: 5000
  txn_table_capacity: 1024
observation:
  queue_depth: 8192
  drop_policy: drop_oldest
  sinks: []
rhai:
  max_operations: 10000
  max_call_depth: 32
pools:
  - name: default
    backends:
      - address: "127.0.0.1:5300"
control:
  listen_address: "127.0.0.1:5199"
"#;
        assert!(load_yaml(yaml).is_err());
    }

    #[test]
    fn accept_with_dnstap_extra_config() {
        let yaml = include_str!("../../../tests/fixtures/config/with-dnstap-extra.yaml");
        let cfg = load_yaml(yaml).unwrap();
        assert!(validate(&cfg).ok);
        let snap = conduit_events::compile_from_config(&cfg, None);
        assert!(snap.enabled);
        assert!(snap.sinks[0].extra_fields.len() >= 3);
    }

    #[test]
    fn accept_with_dnstap_filters_config() {
        let yaml = include_str!("../../../tests/fixtures/config/with-dnstap-filters.yaml");
        let cfg = load_yaml(yaml).unwrap();
        let result = validate(&cfg);
        assert!(result.ok, "errors: {:?}", result.errors);
        let snap = conduit_events::compile_from_config(&cfg, None);
        assert_eq!(snap.sinks[0].filters.selectors.len(), 2);
        assert_eq!(snap.sinks[0].filters.tag_required.as_deref(), Some("audit"));
    }

    #[test]
    fn accept_with_dnstap_sample_config() {
        let yaml = include_str!("../../../tests/fixtures/config/with-dnstap-sample.yaml");
        let cfg = load_yaml(yaml).unwrap();
        assert!(validate(&cfg).ok);
        let snap = conduit_events::compile_from_config(&cfg, None);
        assert!((snap.sinks[0].filters.sample_percent - 10.0).abs() < f64::EPSILON);
        assert_eq!(snap.sinks[0].filters.pool.as_deref(), Some("default"));
    }

    #[test]
    fn reject_invalid_sample_percent() {
        let yaml = include_str!("../../../tests/fixtures/config/with-dnstap-sample.yaml");
        let mut cfg = load_yaml(yaml).unwrap();
        cfg.events.as_mut().unwrap().sinks[0]
            .filters
            .as_mut()
            .unwrap()
            .sample_percent = Some(101.0);
        let result = validate(&cfg);
        assert!(!result.ok);
        assert!(result.errors.iter().any(|e| e.contains("sample_percent")));
    }

    #[test]
    fn accept_with_metrics_prometheus_config() {
        let yaml = include_str!("../../../tests/fixtures/config/with-metrics-prometheus.yaml");
        let cfg = load_yaml(yaml).unwrap();
        assert!(validate(&cfg).ok, "{:?}", validate(&cfg).errors);
        let (m, _) = conduit_metrics::compile_from_config(&cfg);
        assert!(m.enabled);
        assert!(m.prometheus_listen.is_some());
    }

    #[test]
    fn accept_metrics_disabled_config() {
        let yaml = include_str!("../../../tests/fixtures/config/metrics-disabled.yaml");
        let cfg = load_yaml(yaml).unwrap();
        assert!(validate(&cfg).ok);
        let (m, _) = conduit_metrics::compile_from_config(&cfg);
        assert!(!m.enabled);
    }

    #[test]
    fn accept_with_tracing_selectors_config() {
        let yaml = include_str!("../../../tests/fixtures/config/with-tracing-selectors.yaml");
        let cfg = load_yaml(yaml).unwrap();
        assert!(validate(&cfg).ok);
        let (_, t) = conduit_metrics::compile_from_config(&cfg);
        assert!(t.enabled);
    }

    #[test]
    fn reject_invalid_tracing_sample_percent() {
        let yaml = include_str!("../../../tests/fixtures/config/with-tracing-selectors.yaml");
        let mut cfg = load_yaml(yaml).unwrap();
        cfg.tracing
            .as_mut()
            .unwrap()
            .activation
            .as_mut()
            .unwrap()
            .sample_percent = Some(101.0);
        let result = validate(&cfg);
        assert!(!result.ok);
        assert!(result.errors.iter().any(|e| e.contains("sample_percent")));
    }

    #[test]
    fn reject_every_nth_selector_in_events_filters() {
        let yaml = include_str!("../../../tests/fixtures/config/with-dnstap-filters.yaml");
        let mut cfg = load_yaml(yaml).unwrap();
        cfg.events.as_mut().unwrap().sinks[0]
            .filters
            .as_mut()
            .unwrap()
            .selectors
            .push(conduit_proto::config::Selector {
                r#type: "every_nth_worker".into(),
                value: "4".into(),
                key: None,
                key_from: None,
            });
        let result = validate(&cfg);
        assert!(!result.ok);
        assert!(result
            .errors
            .iter()
            .any(|e| e.contains("only valid in rules selectors")));
    }

    #[test]
    fn reject_every_nth_selector_in_tracing_activation() {
        let yaml = include_str!("../../../tests/fixtures/config/with-tracing-selectors.yaml");
        let mut cfg = load_yaml(yaml).unwrap();
        cfg.tracing
            .as_mut()
            .unwrap()
            .activation
            .as_mut()
            .unwrap()
            .selectors
            .push(conduit_proto::config::Selector {
                r#type: "every_nth_global".into(),
                value: "8".into(),
                key: None,
                key_from: None,
            });
        let result = validate(&cfg);
        assert!(!result.ok);
        assert!(result
            .errors
            .iter()
            .any(|e| e.contains("only valid in rules selectors")));
    }

    #[test]
    fn accept_with_sample_key_config() {
        let yaml = include_str!("../../../tests/fixtures/config/with-sample-key.yaml");
        let cfg = load_yaml(yaml).unwrap();
        let result = validate(&cfg);
        assert!(result.ok, "{:?}", result.errors);
    }

    #[test]
    fn reject_sample_percent_key_and_key_from_together() {
        let yaml = include_str!("../../../tests/fixtures/config/with-sample-key.yaml");
        let mut cfg = load_yaml(yaml).unwrap();
        cfg.rules.as_mut().unwrap().rules[0].selectors[1].key = Some("x".into());
        cfg.rules.as_mut().unwrap().rules[0].selectors[1].key_from = Some("qname".into());
        let result = validate(&cfg);
        assert!(!result.ok);
        assert!(result
            .errors
            .iter()
            .any(|e| e.contains("mutually exclusive")));
    }

    #[test]
    fn reject_rule_name_key_from_on_tracing_selector() {
        let yaml = include_str!("../../../tests/fixtures/config/with-tracing-selectors.yaml");
        let mut cfg = load_yaml(yaml).unwrap();
        cfg.tracing
            .as_mut()
            .unwrap()
            .activation
            .as_mut()
            .unwrap()
            .selectors
            .push(conduit_proto::config::Selector {
                r#type: "sample_percent".into(),
                value: "10".into(),
                key: None,
                key_from: Some("rule_name".into()),
            });
        let result = validate(&cfg);
        assert!(!result.ok);
        assert!(result
            .errors
            .iter()
            .any(|e| e.contains("only valid on rule selectors")));
    }

    #[test]
    fn reject_unknown_extra_field() {
        let yaml = include_str!("../../../tests/fixtures/config/with-dnstap.yaml");
        let mut cfg = load_yaml(yaml).unwrap();
        cfg.events.as_mut().unwrap().sinks[0]
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
        let sink = &mut cfg.events.as_mut().unwrap().sinks[0];
        sink.extra_tags = vec!["vip".into()];
        let result = validate(&cfg);
        assert!(!result.ok);
        assert!(result.errors.iter().any(|e| e.contains("extra_tags")));
    }

    #[test]
    fn reject_invalid_sink_type_and_emit() {
        let yaml = include_str!("../../../tests/fixtures/config/minimal.yaml");
        let mut cfg = load_yaml(yaml).unwrap();
        cfg.events
            .as_mut()
            .unwrap()
            .sinks
            .push(conduit_proto::config::EventSink {
                r#type: "syslog".into(),
                export_id: "x".into(),
                destinations: vec!["unix:/tmp/x".into()],
                emit: vec!["bogus".into()],
                filters: None,
                extra_fields: vec![],
                extra_tags: vec![],
                name: None,
                connect_retry: None,
            });
        let result = validate(&cfg);
        assert!(!result.ok);
        assert!(result.errors.iter().any(|e| e.contains("dnstap")));
        assert!(result.errors.iter().any(|e| e.contains("emit")));
    }

    #[test]
    fn reject_duplicate_sink_names() {
        let yaml = include_str!("../../../tests/fixtures/config/with-dnstap.yaml");
        let mut cfg = load_yaml(yaml).unwrap();
        let second = cfg.events.as_mut().unwrap().sinks[0].clone();
        cfg.events.as_mut().unwrap().sinks.push(second);
        let result = validate(&cfg);
        assert!(!result.ok);
        assert!(result.errors.iter().any(|e| e.contains("duplicates")));
    }

    #[test]
    fn reject_empty_sink_name() {
        let yaml = include_str!("../../../tests/fixtures/config/with-dnstap.yaml");
        let mut cfg = load_yaml(yaml).unwrap();
        cfg.events.as_mut().unwrap().sinks[0].name = Some(String::new());
        let result = validate(&cfg);
        assert!(!result.ok);
        assert!(result.errors.iter().any(|e| e.contains("name")));
    }

    #[test]
    fn reject_invalid_connect_retry_multiplier() {
        let yaml = include_str!("../../../tests/fixtures/config/with-dnstap.yaml");
        let mut cfg = load_yaml(yaml).unwrap();
        cfg.events.as_mut().unwrap().sinks[0].connect_retry =
            Some(conduit_proto::config::ConnectRetry {
                initial_ms: 1000,
                max_ms: 30_000,
                multiplier: 0.5,
                max_elapsed_ms: 0,
                jitter: true,
            });
        let result = validate(&cfg);
        assert!(!result.ok);
        assert!(result.errors.iter().any(|e| e.contains("multiplier")));
    }

    #[test]
    fn accept_custom_connect_retry() {
        let yaml = include_str!("../../../tests/fixtures/config/with-dnstap.yaml");
        let mut cfg = load_yaml(yaml).unwrap();
        cfg.events.as_mut().unwrap().sinks[0].connect_retry =
            Some(conduit_proto::config::ConnectRetry {
                initial_ms: 250,
                max_ms: 8000,
                multiplier: 2.0,
                max_elapsed_ms: 0,
                jitter: false,
            });
        let result = validate(&cfg);
        assert!(result.ok, "{:?}", result.errors);
        let snap = conduit_events::compile_from_config(&cfg, None);
        assert_eq!(snap.sinks[0].connect_retry.initial_ms, 250);
        assert!(!snap.sinks[0].connect_retry.jitter);
    }

    #[test]
    fn accept_forward_sources_v4_fixture() {
        let yaml = include_str!("../../../tests/fixtures/config/forward-sources-v4.yaml");
        let cfg = load_yaml(yaml).unwrap();
        assert!(validate(&cfg).ok, "{:?}", validate(&cfg).errors);
    }

    #[test]
    fn reject_empty_sources_v4_entry() {
        let yaml = include_str!("../../../tests/fixtures/config/minimal.yaml");
        let mut cfg = load_yaml(yaml).unwrap();
        cfg.forward.as_mut().unwrap().sources_v4 = vec!["".into()];
        let result = validate(&cfg);
        assert!(!result.ok);
        assert!(result.errors.iter().any(|e| e.contains("sources_v4")));
    }

    #[test]
    fn accept_ipv6_upstream_backend() {
        let yaml = include_str!("../../../tests/fixtures/config/forward-sources-v6.yaml");
        let cfg = load_yaml(yaml).unwrap();
        assert!(validate(&cfg).ok, "{:?}", validate(&cfg).errors);
    }

    #[test]
    fn reject_duplicate_pool_names() {
        let yaml = include_str!("../../../tests/fixtures/config/minimal.yaml");
        let mut cfg = load_yaml(yaml).unwrap();
        let second = cfg.pools[0].clone();
        cfg.pools.push(second);
        let result = validate(&cfg);
        assert!(!result.ok);
        assert!(result.errors.iter().any(|e| e.contains("duplicate pool")));
    }

    #[test]
    fn reject_duplicate_rule_names() {
        let yaml = include_str!("../../../tests/fixtures/config/with-rules.yaml");
        let mut cfg = load_yaml(yaml).unwrap();
        let second = cfg.rules.as_ref().unwrap().rules[0].clone();
        cfg.rules.as_mut().unwrap().rules.push(second);
        let result = validate(&cfg);
        assert!(!result.ok);
        assert!(result.errors.iter().any(|e| e.contains("duplicate rule")));
    }

    #[test]
    fn accept_set_source_v4_on_request_rule() {
        let yaml = include_str!("../../../tests/fixtures/config/with-rules-set-source-v4.yaml");
        let cfg = load_yaml(yaml).unwrap();
        assert!(validate(&cfg).ok, "{:?}", validate(&cfg).errors);
    }

    #[test]
    fn reject_set_source_v4_on_response_hook() {
        let yaml = include_str!("../../../tests/fixtures/config/with-rules-set-source-v4.yaml");
        let mut cfg = load_yaml(yaml).unwrap();
        cfg.rules.as_mut().unwrap().rules[0].hook = "response".into();
        let result = validate(&cfg);
        assert!(!result.ok);
        assert!(result
            .errors
            .iter()
            .any(|e| e.contains("only valid on request hook")));
    }

    #[test]
    fn reject_set_source_v4_not_in_configured_sources() {
        let yaml = include_str!("../../../tests/fixtures/config/with-rules-set-source-v4.yaml");
        let mut cfg = load_yaml(yaml).unwrap();
        cfg.rules.as_mut().unwrap().rules[0].actions[1].value = "192.0.2.99".into();
        let result = validate(&cfg);
        assert!(!result.ok);
        assert!(result
            .errors
            .iter()
            .any(|e| e.contains("not in configured sources_v4")));
    }

    #[test]
    fn reject_set_source_v4_without_configured_sources() {
        let yaml = include_str!("../../../tests/fixtures/config/with-rules.yaml");
        let mut cfg = load_yaml(yaml).unwrap();
        cfg.rules.as_mut().unwrap().rules[0]
            .actions
            .push(conduit_proto::config::Action {
                r#type: "set_source_v4".into(),
                value: "127.0.0.1".into(),
            });
        let result = validate(&cfg);
        assert!(!result.ok);
        assert!(result
            .errors
            .iter()
            .any(|e| e.contains("requires forward.sources_v4")));
    }

    #[test]
    fn accept_set_retry_source_v4_on_response_hook() {
        let yaml =
            include_str!("../../../tests/fixtures/config/with-rules-set-retry-source-v4.yaml");
        let cfg = load_yaml(yaml).unwrap();
        assert!(validate(&cfg).ok, "{:?}", validate(&cfg).errors);
    }

    #[test]
    fn reject_set_retry_source_v4_not_in_configured_sources() {
        let yaml =
            include_str!("../../../tests/fixtures/config/with-rules-set-retry-source-v4.yaml");
        let mut cfg = load_yaml(yaml).unwrap();
        cfg.rules.as_mut().unwrap().rules[0].actions[0].value = "192.0.2.99".into();
        let result = validate(&cfg);
        assert!(!result.ok);
        assert!(result
            .errors
            .iter()
            .any(|e| e.contains("not in configured sources_v4")));
    }

    #[test]
    fn reject_clear_tag_without_key() {
        let yaml = include_str!("../../../tests/fixtures/config/with-rules.yaml");
        let mut cfg = load_yaml(yaml).unwrap();
        cfg.rules.as_mut().unwrap().rules[0]
            .actions
            .push(conduit_proto::config::Action {
                r#type: "clear_tag".into(),
                value: "".into(),
            });
        let result = validate(&cfg);
        assert!(!result.ok);
        assert!(result
            .errors
            .iter()
            .any(|e| e.contains("clear_tag requires a non-empty tag key")));
    }
}
