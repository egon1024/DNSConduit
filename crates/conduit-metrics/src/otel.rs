//! OTEL metrics periodic push (OTLP HTTP).

use crate::export::render_prometheus;
use crate::MetricsHub;
use conduit_events::EventHub;
use opentelemetry::KeyValue;
use opentelemetry_otlp::WithExportConfig;
use opentelemetry_sdk::metrics::data::{DataPoint, Metric, ResourceMetrics, ScopeMetrics, Sum};
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

/// Map Prometheus text exposition (counters) into OTEL `ResourceMetrics` for OTLP export.
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
            }
            continue;
        }
        if line.is_empty() || line.starts_with('#') {
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
