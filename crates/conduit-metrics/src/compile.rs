//! Compile metrics and tracing sections from config.

use conduit_events::{
    compile_selectors, hash_sample, parse_sample_rate, validate_selector_type, CompiledSelector,
    SelectorMatchCtx,
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
    pub sample_rate: f64,
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
        .map(|a| compile_selectors(&a.selectors))
        .unwrap_or_default();
    let sample_rate = activation
        .and_then(|a| parse_sample_rate(a.sample_rate).ok())
        .unwrap_or(1.0);
    let tag_required = activation.and_then(|a| a.tag.as_ref().filter(|s| !s.is_empty()).cloned());
    CompiledTracing {
        enabled: true,
        activation: CompiledTraceActivation {
            tag_required,
            selectors,
            sample_rate,
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
            qname,
            qtype_label,
            rcode_label,
            tag_has,
        };
        if !activation.selectors.iter().all(|s| s.matches_ctx(&ctx)) {
            return false;
        }
    }
    hash_sample(txn_id, activation.sample_rate)
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
                    if let Err(e) = validate_selector_type(sel.r#type.as_str()) {
                        errors.push(format!("tracing.activation.selectors[{j}]: {e}"));
                    }
                }
                if let Some(rate) = a.sample_rate {
                    if let Err(e) = parse_sample_rate(Some(rate)) {
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
            sample_rate: 0.5,
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
                    sample_rate: Some(1.0),
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
}
