//! Compile metrics and tracing sections from config.

use conduit_events::{
    compile_selectors, hash_sample, parse_sample_percent, validate_non_rule_selector_type,
    CompiledSelector, SelectorMatchCtx,
};
use conduit_proto::config::{Config, MetricsConfig, TracingConfig};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuiltinProfile {
    Full,
    Minimal,
    Off,
}

#[derive(Debug, Clone)]
pub struct CompiledMetrics {
    pub enabled: bool,
    pub profile: BuiltinProfile,
    pub prometheus_listen: Option<String>,
    pub prometheus_path: String,
    pub otel_endpoint: Option<String>,
    pub otel_push_interval_ms: u32,
    pub otel_resource_attributes: Vec<(String, String)>,
    pub otel_allow_invalid_certs: bool,
    /// OTLP HTTP headers (for future auth); not sent when empty.
    pub otel_headers: Vec<(String, String)>,
}

impl Default for CompiledMetrics {
    fn default() -> Self {
        Self {
            enabled: false,
            profile: BuiltinProfile::Off,
            prometheus_listen: None,
            prometheus_path: "/metrics".into(),
            otel_endpoint: None,
            otel_push_interval_ms: 15_000,
            otel_resource_attributes: Vec::new(),
            otel_allow_invalid_certs: false,
            otel_headers: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct CompiledTracing {
    pub enabled: bool,
    pub activation: CompiledTraceActivation,
    pub log_json: bool,
}

#[derive(Debug, Clone, Default)]
pub struct CompiledTraceActivation {
    pub tag_required: Option<String>,
    pub selectors: Vec<CompiledSelector>,
    pub sample_percent: f64,
}

pub fn compile_from_config(config: &Config) -> (CompiledMetrics, CompiledTracing) {
    (
        compile_metrics(config.metrics.as_ref()),
        compile_tracing(config.tracing.as_ref()),
    )
}

fn compile_metrics(m: Option<&MetricsConfig>) -> CompiledMetrics {
    let Some(m) = m else {
        return CompiledMetrics::default();
    };
    if !m.enabled {
        return CompiledMetrics {
            enabled: false,
            profile: BuiltinProfile::Off,
            ..Default::default()
        };
    }
    let profile = match m.profile.as_str() {
        "minimal" => BuiltinProfile::Minimal,
        "off" => BuiltinProfile::Off,
        _ => BuiltinProfile::Full,
    };
    let prometheus_listen = m
        .prometheus
        .as_ref()
        .filter(|p| !p.listen_address.is_empty())
        .map(|p| p.listen_address.clone());
    let prometheus_path = m
        .prometheus
        .as_ref()
        .map(|p| {
            if p.path.is_empty() {
                "/metrics".into()
            } else {
                p.path.clone()
            }
        })
        .unwrap_or_else(|| "/metrics".into());
    let otel = m.otel.as_ref();
    CompiledMetrics {
        enabled: profile != BuiltinProfile::Off,
        profile,
        prometheus_listen,
        prometheus_path,
        otel_endpoint: otel
            .filter(|o| !o.endpoint.is_empty())
            .map(|o| o.endpoint.clone()),
        otel_push_interval_ms: otel.map(|o| o.push_interval_ms).unwrap_or(15_000).max(1000),
        otel_resource_attributes: otel
            .map(|o| {
                o.resource_attributes
                    .iter()
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect()
            })
            .unwrap_or_default(),
        otel_allow_invalid_certs: otel.map(|o| o.allow_invalid_certs).unwrap_or(false),
        otel_headers: otel
            .map(|o| {
                o.headers
                    .iter()
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect()
            })
            .unwrap_or_default(),
    }
}

fn compile_tracing(t: Option<&TracingConfig>) -> CompiledTracing {
    let Some(t) = t else {
        return CompiledTracing::default();
    };
    if !t.enabled {
        return CompiledTracing::default();
    }
    let activation = t.activation.as_ref();
    let selectors = activation
        .map(|a| {
            let allowed: Vec<_> = a
                .selectors
                .iter()
                .filter(|sel| validate_non_rule_selector_type(sel.r#type.as_str()).is_ok())
                .cloned()
                .collect();
            compile_selectors(&allowed)
        })
        .unwrap_or_default();
    let sample_percent = activation
        .and_then(|a| parse_sample_percent(a.sample_percent).ok())
        .unwrap_or(100.0);
    let tag_required = activation.and_then(|a| a.tag.as_ref().filter(|s| !s.is_empty()).cloned());
    CompiledTracing {
        enabled: true,
        activation: CompiledTraceActivation {
            tag_required,
            selectors,
            sample_percent,
        },
        log_json: t.output.as_ref().map(|o| o.log_json).unwrap_or(false),
    }
}

/// Whether pipeline tracing should be enabled for this transaction (after RequestRules).
pub fn trace_activation_matches(
    activation: &CompiledTraceActivation,
    txn_id: u64,
    qname: Option<&str>,
    qtype_label: Option<String>,
    rcode_label: Option<String>,
    tag_has: &dyn Fn(&str) -> bool,
) -> bool {
    if let Some(ref key) = activation.tag_required {
        if !tag_has(key) {
            return false;
        }
    }
    if !activation.selectors.is_empty() {
        let ctx = SelectorMatchCtx {
            txn_id,
            global_query_index: 0,
            qname,
            qtype_label,
            rcode_label,
            tag_has,
        };
        if !activation.selectors.iter().all(|s| s.matches_ctx(&ctx)) {
            return false;
        }
    }
    hash_sample(txn_id, activation.sample_percent / 100.0)
}

pub fn validate_metrics_tracing(cfg: &Config) -> Vec<String> {
    let mut errors = Vec::new();
    if let Some(m) = &cfg.metrics {
        if m.enabled {
            if !matches!(m.profile.as_str(), "full" | "minimal" | "off" | "") {
                errors.push(format!(
                    "metrics.profile '{}' must be full, minimal, or off",
                    m.profile
                ));
            }
            if let Some(p) = &m.prometheus {
                if !p.listen_address.is_empty()
                    && p.listen_address.parse::<std::net::SocketAddr>().is_err()
                {
                    errors.push(format!(
                        "metrics.prometheus.listen_address '{}' is not a valid socket address",
                        p.listen_address
                    ));
                }
            }
            if let Some(o) = &m.otel {
                if !o.endpoint.is_empty() && !o.endpoint.starts_with("http") {
                    errors
                        .push("metrics.otel.endpoint must be an http(s) URL for OTLP HTTP".into());
                }
                if o.push_interval_ms > 0 && o.push_interval_ms < 1000 {
                    errors.push("metrics.otel.push_interval_ms must be >= 1000".into());
                }
            }
        }
    }
    if let Some(t) = &cfg.tracing {
        if t.enabled {
            if let Some(a) = &t.activation {
                for (j, sel) in a.selectors.iter().enumerate() {
                    if let Err(e) = validate_non_rule_selector_type(sel.r#type.as_str()) {
                        errors.push(format!("tracing.activation.selectors[{j}]: {e}"));
                    }
                    if sel.r#type == "sample_percent"
                        && conduit_events::parse_selector_sample_percent(&sel.value).is_err()
                    {
                        errors.push(format!(
                            "tracing.activation.selectors[{j}] sample_percent must be in [0, 100]"
                        ));
                    }
                }
                if let Some(percent) = a.sample_percent {
                    if let Err(e) = parse_sample_percent(Some(percent)) {
                        errors.push(format!("tracing.activation: {e}"));
                    }
                }
            }
        }
    }
    errors
}

#[cfg(test)]
mod tests {
    use super::*;
    use conduit_proto::config::{Selector, TracingActivation, TracingConfig, TracingOutput};

    #[test]
    fn trace_sample_stable() {
        let activation = CompiledTraceActivation {
            sample_percent: 50.0,
            ..Default::default()
        };
        let id = 42u64;
        let a = trace_activation_matches(&activation, id, None, None, None, &|_| true);
        let b = trace_activation_matches(&activation, id, None, None, None, &|_| true);
        assert_eq!(a, b);
    }

    #[test]
    fn compile_tracing_disabled_by_default() {
        let cfg = Config {
            schema_version: 1,
            listeners: None,
            forward: None,
            orchestrator: None,
            events: None,
            rhai: None,
            pools: vec![],
            control: None,
            rules: None,
            logging: None,
            data_sources: vec![],
            metrics: None,
            tracing: None,
        };
        let (_, t) = compile_from_config(&cfg);
        assert!(!t.enabled);
    }

    #[test]
    fn compile_tracing_with_tag() {
        let cfg = Config {
            schema_version: 1,
            tracing: Some(TracingConfig {
                enabled: true,
                activation: Some(TracingActivation {
                    tag: Some("trace".into()),
                    selectors: vec![Selector {
                        r#type: "qtype".into(),
                        value: "A".into(),
                    }],
                    sample_percent: Some(100.0),
                }),
                output: Some(TracingOutput { log_json: true }),
            }),
            listeners: None,
            forward: None,
            orchestrator: None,
            events: None,
            rhai: None,
            pools: vec![],
            control: None,
            rules: None,
            logging: None,
            data_sources: vec![],
            metrics: None,
        };
        let (_, t) = compile_from_config(&cfg);
        assert!(t.enabled);
        assert!(t.log_json);
        assert_eq!(t.activation.tag_required.as_deref(), Some("trace"));
    }

    #[test]
    fn compile_otel_tls_and_headers() {
        use conduit_proto::config::{MetricsConfig, OtelMetricsConfig};
        let cfg = Config {
            schema_version: 1,
            metrics: Some(MetricsConfig {
                enabled: true,
                profile: "full".into(),
                prometheus: None,
                otel: Some(OtelMetricsConfig {
                    endpoint: "https://collector.example/v1/metrics".into(),
                    push_interval_ms: 5000,
                    resource_attributes: Default::default(),
                    allow_invalid_certs: true,
                    headers: [("X-Test".to_string(), "1".to_string())]
                        .into_iter()
                        .collect(),
                }),
            }),
            listeners: None,
            forward: None,
            orchestrator: None,
            events: None,
            rhai: None,
            pools: vec![],
            control: None,
            rules: None,
            logging: None,
            data_sources: vec![],
            tracing: None,
        };
        let (m, _) = compile_from_config(&cfg);
        assert_eq!(
            m.otel_endpoint.as_deref(),
            Some("https://collector.example/v1/metrics")
        );
        assert!(m.otel_allow_invalid_certs);
        assert_eq!(m.otel_headers, vec![("X-Test".into(), "1".into())]);
    }
}
