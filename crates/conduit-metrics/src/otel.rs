//! OTEL metrics periodic push (OTLP HTTP) — Prometheus text parity for counters, gauges, histograms.

use crate::export::render_prometheus;
use crate::task::OtelPushHandle;
use crate::MetricsHub;
use async_trait::async_trait;
use conduit_events::EventHub;
use http::header::{HeaderName, HeaderValue};
use http::{Request, Response};
use opentelemetry::KeyValue;
use opentelemetry_http::Bytes;
use opentelemetry_http::HttpClient;
use opentelemetry_otlp::{WithExportConfig, WithHttpConfig};
use opentelemetry_sdk::metrics::data::{
    DataPoint, Histogram, Metric, ResourceMetrics, ScopeMetrics, Sum,
};
use opentelemetry_sdk::metrics::exporter::PushMetricExporter;
use opentelemetry_sdk::metrics::Temporality;
use opentelemetry_sdk::Resource;
use std::borrow::Cow;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

/// Settings for OTLP HTTP metrics push.
#[derive(Debug, Clone)]
pub struct OtelPushSettings {
    pub endpoint: String,
    pub push_interval_ms: u32,
    pub resource_attributes: Vec<(String, String)>,
    pub allow_invalid_certs: bool,
    /// Reserved for future auth headers; ignored when empty.
    pub headers: Vec<(String, String)>,
}

pub fn spawn_otel_push(
    settings: OtelPushSettings,
    hub: Arc<MetricsHub>,
    observation: Arc<EventHub>,
) -> OtelPushHandle {
    let (shutdown_tx, mut shutdown_rx) = tokio::sync::oneshot::channel();
    let join = tokio::spawn(async move {
        let endpoint = settings.endpoint.clone();
        let exporter = match build_metric_exporter(&settings) {
            Ok(e) => e,
            Err(e) => {
                tracing::error!(error = %e, %endpoint, "failed to build OTLP metric exporter");
                return;
            }
        };

        let mut resource_kv: Vec<KeyValue> = settings
            .resource_attributes
            .into_iter()
            .map(|(k, v)| KeyValue::new(k, v))
            .collect();
        resource_kv.push(KeyValue::new("service.name", "conduit"));

        let interval = Duration::from_millis(settings.push_interval_ms.max(1000) as u64);
        loop {
            tokio::select! {
                _ = &mut shutdown_rx => break,
                _ = tokio::time::sleep(interval) => {
                    if hub.metrics_enabled() {
                        let obs = observation.sink_metrics_snapshot();
                        let prom_text = render_prometheus(hub.as_ref(), &obs);
                        let mut resource_metrics =
                            prometheus_text_to_resource_metrics(&prom_text, resource_kv.clone());
                        if let Err(e) = exporter.export(&mut resource_metrics).await {
                            tracing::warn!(error = %e, %endpoint, "otel metrics push failed");
                        } else {
                            tracing::debug!(%endpoint, "otel metrics push ok");
                        }
                    }
                }
            }
        }
        tracing::debug!(%endpoint, "otel metrics push stopped");
    });
    OtelPushHandle::new(shutdown_tx, join)
}

fn build_metric_exporter(
    settings: &OtelPushSettings,
) -> Result<opentelemetry_otlp::MetricExporter, String> {
    let headers: HashMap<String, String> = settings.headers.iter().cloned().collect();
    let client = if headers.is_empty() {
        OtelHttpClient::Plain(build_reqwest_client(settings.allow_invalid_certs)?)
    } else {
        OtelHttpClient::WithHeaders {
            inner: build_reqwest_client(settings.allow_invalid_certs)?,
            headers,
        }
    };
    opentelemetry_otlp::MetricExporter::builder()
        .with_http()
        .with_endpoint(settings.endpoint.clone())
        .with_http_client(client)
        .build()
        .map_err(|e| e.to_string())
}

#[derive(Debug)]
enum OtelHttpClient {
    Plain(reqwest::Client),
    WithHeaders {
        inner: reqwest::Client,
        headers: HashMap<String, String>,
    },
}

#[async_trait]
impl HttpClient for OtelHttpClient {
    async fn send(
        &self,
        request: Request<Vec<u8>>,
    ) -> Result<Response<Bytes>, opentelemetry_http::HttpError> {
        match self {
            OtelHttpClient::Plain(client) => client.send(request).await,
            OtelHttpClient::WithHeaders { inner, headers } => {
                let (parts, body) = request.into_parts();
                let mut req = Request::from_parts(parts, body);
                for (key, value) in headers {
                    if let (Ok(name), Ok(val)) = (
                        HeaderName::try_from(key.as_str()),
                        HeaderValue::from_str(value),
                    ) {
                        req.headers_mut().insert(name, val);
                    }
                }
                inner.send(req).await
            }
        }
    }
}

fn build_reqwest_client(allow_invalid_certs: bool) -> Result<reqwest::Client, String> {
    let mut builder = reqwest::Client::builder();
    if allow_invalid_certs {
        builder = builder.danger_accept_invalid_certs(true);
    }
    builder.build().map_err(|e| format!("reqwest client: {e}"))
}

/// Push one OTLP metrics payload (used by integration tests).
pub async fn push_metrics_once(
    hub: &MetricsHub,
    observation: &EventHub,
    settings: &OtelPushSettings,
) -> Result<(), String> {
    let exporter = build_metric_exporter(settings)?;
    let mut resource_kv: Vec<KeyValue> = settings
        .resource_attributes
        .iter()
        .map(|(k, v)| KeyValue::new(k.clone(), v.clone()))
        .collect();
    resource_kv.push(KeyValue::new("service.name", "conduit"));
    let obs = observation.sink_metrics_snapshot();
    let prom_text = render_prometheus(hub, &obs);
    let mut resource_metrics = prometheus_text_to_resource_metrics(&prom_text, resource_kv);
    exporter
        .export(&mut resource_metrics)
        .await
        .map_err(|e| e.to_string())
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
        reg.record_response("ln", "udp", Some(0), &addr, Some("forward"));
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

    #[test]
    fn build_metric_exporter_accepts_http_endpoint() {
        let settings = OtelPushSettings {
            endpoint: "http://127.0.0.1:4318/v1/metrics".into(),
            push_interval_ms: 15_000,
            resource_attributes: vec![],
            allow_invalid_certs: false,
            headers: vec![],
        };
        assert!(build_metric_exporter(&settings).is_ok());
    }

    #[test]
    fn build_metric_exporter_accepts_https_endpoint() {
        let settings = OtelPushSettings {
            endpoint: "https://collector.example/v1/metrics".into(),
            push_interval_ms: 15_000,
            resource_attributes: vec![],
            allow_invalid_certs: false,
            headers: vec![],
        };
        assert!(build_metric_exporter(&settings).is_ok());
    }
}
