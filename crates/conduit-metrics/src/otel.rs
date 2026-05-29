//! OTEL metrics periodic push (OTLP HTTP) — Prometheus text parity for counters, gauges, histograms.

use crate::export::render_prometheus;
use crate::MetricsHub;
use conduit_events::EventHub;
use opentelemetry::KeyValue;
use opentelemetry_otlp::WithExportConfig;
use opentelemetry_sdk::metrics::data::{
    DataPoint, Histogram, Metric, ResourceMetrics, ScopeMetrics, Sum,
};
use opentelemetry_sdk::metrics::exporter::PushMetricExporter;
use opentelemetry_sdk::metrics::Temporality;
use opentelemetry_sdk::Resource;
use std::borrow::Cow;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

pub fn spawn_otel_push(
    endpoint: String,
    interval_ms: u32,
    resource_attributes: Vec<(String, String)>,
    hub: Arc<MetricsHub>,
    observation: Arc<EventHub>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let exporter = match opentelemetry_otlp::MetricExporter::builder()
            .with_http()
            .with_endpoint(endpoint.clone())
            .build()
        {
            Ok(e) => e,
            Err(e) => {
                tracing::error!(error = %e, endpoint = %endpoint, "failed to build OTLP metric exporter");
                return;
            }
        };

        let mut resource_kv: Vec<KeyValue> = resource_attributes
            .into_iter()
            .map(|(k, v)| KeyValue::new(k, v))
            .collect();
        resource_kv.push(KeyValue::new("service.name", "conduit"));

        let interval = Duration::from_millis(interval_ms.max(1000) as u64);
        loop {
            if hub.metrics_enabled() {
                let obs = observation.sink_metrics_snapshot();
                let prom_text = render_prometheus(hub.as_ref(), &obs);
                let mut resource_metrics =
                    prometheus_text_to_resource_metrics(&prom_text, resource_kv.clone());
                if let Err(e) = exporter.export(&mut resource_metrics).await {
                    tracing::warn!(error = %e, endpoint = %endpoint, "otel metrics push failed");
                } else {
                    tracing::debug!(endpoint = %endpoint, "otel metrics push ok");
                }
            }
            tokio::time::sleep(interval).await;
        }
    })
}

/// Map Prometheus text exposition into OTEL `ResourceMetrics` for OTLP export.
fn prometheus_text_to_resource_metrics(
    text: &str,
    resource_attributes: Vec<KeyValue>,
) -> ResourceMetrics {
    let mut scope = ScopeMetrics {
        scope: opentelemetry::InstrumentationScope::builder("conduit").build(),
        metrics: Vec::new(),
    };

    let mut current_name = String::new();
    let mut current_type = String::new();
    let mut histogram_buckets: Vec<f64> = Vec::new();

    for line in text.lines() {
        if let Some(name) = line.strip_prefix("# HELP ") {
            if let Some((metric, _)) = name.split_once(' ') {
                current_name = metric.to_string();
            }
            continue;
        }
        if let Some(typ) = line.strip_prefix("# TYPE ") {
            if let Some((metric, ty)) = typ.split_once(' ') {
                current_name = metric.to_string();
                current_type = ty.to_string();
                histogram_buckets.clear();
            }
            continue;
        }
        if line.starts_with("#") {
            continue;
        }
        if line.is_empty() {
            continue;
        }

        if current_type == "counter" {
            if let Some((head, value)) = line.rsplit_once(' ') {
                if let Ok(v) = value.parse::<u64>() {
                    let (name, attrs) = parse_labels(head);
                    scope.metrics.push(counter_metric(
                        if name.is_empty() {
                            &current_name
                        } else {
                            &name
                        },
                        v,
                        attrs,
                    ));
                }
            }
        } else if current_type == "gauge" {
            if let Some((head, value)) = line.rsplit_once(' ') {
                if let Ok(v) = value.parse::<f64>() {
                    let (name, attrs) = parse_labels(head);
                    let metric_name = if name.is_empty() {
                        current_name.as_str()
                    } else {
                        &name
                    };
                    if !metric_name.ends_with("_bucket") {
                        scope.metrics.push(gauge_metric(metric_name, v, attrs));
                    }
                }
            }
        } else if current_type == "histogram" {
            if let Some((head, value)) = line.rsplit_once(' ') {
                if head.contains("le=") {
                    if let Some(le) = head.split("le=\"").nth(1).and_then(|s| s.split('"').next()) {
                        if le != "+Inf" {
                            if let Ok(b) = le.parse::<f64>() {
                                histogram_buckets.push(b);
                            }
                        }
                    }
                    if let Ok(cumulative) = value.parse::<u64>() {
                        let (name, attrs) = parse_labels(head);
                        let metric_name = if name.is_empty() {
                            current_name.clone()
                        } else {
                            name
                        };
                        if head.contains("le=\"+Inf\"") {
                            scope.metrics.push(histogram_metric(
                                &metric_name,
                                attrs,
                                &histogram_buckets,
                                cumulative,
                            ));
                            histogram_buckets.clear();
                        }
                    }
                }
            }
        }
    }

    ResourceMetrics {
        resource: Resource::new(resource_attributes),
        scope_metrics: vec![scope],
    }
}

fn parse_labels(head: &str) -> (String, Vec<KeyValue>) {
    let Some((name, rest)) = head.split_once('{') else {
        return (head.to_string(), Vec::new());
    };
    let labels = rest
        .trim_end_matches('}')
        .split(',')
        .filter_map(|pair| {
            let (k, v) = pair.split_once('=')?;
            Some(KeyValue::new(
                k.trim().to_string(),
                v.trim().trim_matches('"').to_string(),
            ))
        })
        .collect();
    (name.to_string(), labels)
}

fn counter_metric(name: &str, value: u64, attrs: Vec<KeyValue>) -> Metric {
    Metric {
        name: Cow::Owned(name.to_string()),
        description: Cow::Borrowed(""),
        unit: Cow::Borrowed(""),
        data: Box::new(Sum {
            data_points: vec![DataPoint {
                attributes: attrs,
                start_time: None,
                time: Some(SystemTime::now()),
                value,
                exemplars: vec![],
            }],
            temporality: Temporality::Cumulative,
            is_monotonic: true,
        }),
    }
}

fn gauge_metric(name: &str, value: f64, attrs: Vec<KeyValue>) -> Metric {
    Metric {
        name: Cow::Owned(name.to_string()),
        description: Cow::Borrowed(""),
        unit: Cow::Borrowed(""),
        data: Box::new(opentelemetry_sdk::metrics::data::Gauge {
            data_points: vec![DataPoint {
                attributes: attrs,
                start_time: None,
                time: Some(SystemTime::now()),
                value,
                exemplars: vec![],
            }],
        }),
    }
}

fn histogram_metric(
    name: &str,
    attrs: Vec<KeyValue>,
    bucket_upper_bounds: &[f64],
    count: u64,
) -> Metric {
    let now = SystemTime::now();
    let bounds: Vec<f64> = bucket_upper_bounds.to_vec();
    let bucket_counts = vec![count; bounds.len() + 1];
    Metric {
        name: Cow::Owned(name.to_string()),
        description: Cow::Borrowed(""),
        unit: Cow::Borrowed("s"),
        data: Box::new(Histogram {
            data_points: vec![opentelemetry_sdk::metrics::data::HistogramDataPoint {
                attributes: attrs,
                start_time: now,
                time: now,
                count,
                bounds,
                bucket_counts,
                min: None,
                max: None,
                sum: 0.0,
                exemplars: vec![],
            }],
            temporality: Temporality::Cumulative,
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prom_counters_map_to_otel() {
        let text = r#"# HELP conduit_queries_total queries
# TYPE conduit_queries_total counter
conduit_queries_total{listener="ln",protocol="udp"} 3
"#;
        let rm = prometheus_text_to_resource_metrics(text, vec![]);
        assert_eq!(rm.scope_metrics.len(), 1);
        assert_eq!(rm.scope_metrics[0].metrics.len(), 1);
        assert_eq!(rm.scope_metrics[0].metrics[0].name, "conduit_queries_total");
    }

    #[test]
    fn prom_gauges_map_to_otel() {
        let text = r#"# TYPE conduit_config_generation gauge
conduit_config_generation 2
"#;
        let rm = prometheus_text_to_resource_metrics(text, vec![]);
        assert!(rm.scope_metrics[0]
            .metrics
            .iter()
            .any(|m| m.name == "conduit_config_generation"));
    }

    #[test]
    fn parity_matrix_core_4b_families() {
        let reg = crate::builtin::BuiltinRegistry::new(true, crate::compile::BuiltinProfile::Full);
        let addr: std::net::SocketAddr = "127.0.0.1:15353".parse().unwrap();
        reg.record_query("ln", "udp", Some(1), Some(1), &addr);
        reg.record_query_by_pool("default");
        reg.record_parse_rejected("wire_error");
        reg.record_response("ln", "udp", Some(0), &addr);
        let prom = crate::builtin::encode_builtin(reg.gather());

        let prom_names: std::collections::HashSet<_> = prom
            .lines()
            .filter_map(|l| l.strip_prefix("# TYPE "))
            .filter_map(|l| l.split_whitespace().next())
            .map(str::to_string)
            .collect();
        let rm = prometheus_text_to_resource_metrics(&prom, vec![]);
        let otel_names: std::collections::HashSet<_> = rm
            .scope_metrics
            .iter()
            .flat_map(|s| s.metrics.iter().map(|m| m.name.to_string()))
            .collect();

        for family in [
            "conduit_queries_total",
            "conduit_queries_by_pool_total",
            "conduit_parse_rejected_total",
            "conduit_responses_total",
            "conduit_build_info",
            "conduit_config_generation",
            "conduit_start_time_seconds",
        ] {
            assert!(prom_names.contains(family), "prom missing {family}");
            assert!(otel_names.contains(family), "otel missing {family}");
        }
    }
}
