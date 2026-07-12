//! OTEL metrics periodic push (OTLP HTTP) — Prometheus family parity for
//! counters, gauges, and histograms (HELP, units, sum/count/buckets).

use crate::export::gather_prometheus_families;
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
    DataPoint, Gauge, Histogram, HistogramDataPoint, Metric, ResourceMetrics, ScopeMetrics, Sum,
};
use opentelemetry_sdk::metrics::exporter::PushMetricExporter;
use opentelemetry_sdk::metrics::Temporality;
use opentelemetry_sdk::Resource;
use prometheus::proto::{MetricFamily, MetricType};
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

        // Fixed start for cumulative series for the life of this push task.
        let series_start = SystemTime::now();
        let interval = Duration::from_millis(settings.push_interval_ms.max(1000) as u64);
        loop {
            tokio::select! {
                _ = &mut shutdown_rx => break,
                _ = tokio::time::sleep(interval) => {
                    if hub.metrics_enabled() {
                        let obs = observation.sink_metrics_snapshot();
                        let families = gather_prometheus_families(hub.as_ref(), &obs);
                        let mut resource_metrics = families_to_resource_metrics(
                            &families,
                            resource_kv.clone(),
                            series_start,
                        );
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
    let families = gather_prometheus_families(hub, &obs);
    let series_start = SystemTime::now();
    let mut resource_metrics = families_to_resource_metrics(&families, resource_kv, series_start);
    exporter
        .export(&mut resource_metrics)
        .await
        .map_err(|e| e.to_string())
}

/// Map Prometheus `MetricFamily` protos into OTEL `ResourceMetrics` for OTLP export.
fn families_to_resource_metrics(
    families: &[MetricFamily],
    resource_attributes: Vec<KeyValue>,
    series_start: SystemTime,
) -> ResourceMetrics {
    let now = SystemTime::now();
    let mut scope = ScopeMetrics {
        scope: opentelemetry::InstrumentationScope::builder("conduit").build(),
        metrics: Vec::new(),
    };

    for family in families {
        let name = family.get_name();
        if name.is_empty() {
            continue;
        }
        let description = family.get_help().to_string();
        let unit = unit_from_metric_name(name);
        match family.get_field_type() {
            MetricType::COUNTER => {
                let data_points: Vec<_> = family
                    .get_metric()
                    .iter()
                    .map(|m| DataPoint {
                        attributes: labels_to_attrs(m.get_label()),
                        start_time: Some(series_start),
                        time: Some(now),
                        value: m.get_counter().get_value() as u64,
                        exemplars: vec![],
                    })
                    .collect();
                if !data_points.is_empty() {
                    scope.metrics.push(Metric {
                        name: Cow::Owned(name.to_string()),
                        description: Cow::Owned(description),
                        unit: Cow::Borrowed(unit),
                        data: Box::new(Sum {
                            data_points,
                            temporality: Temporality::Cumulative,
                            is_monotonic: true,
                        }),
                    });
                }
            }
            MetricType::GAUGE => {
                let data_points: Vec<_> = family
                    .get_metric()
                    .iter()
                    .map(|m| DataPoint {
                        attributes: labels_to_attrs(m.get_label()),
                        start_time: None,
                        time: Some(now),
                        value: m.get_gauge().get_value(),
                        exemplars: vec![],
                    })
                    .collect();
                if !data_points.is_empty() {
                    scope.metrics.push(Metric {
                        name: Cow::Owned(name.to_string()),
                        description: Cow::Owned(description),
                        unit: Cow::Borrowed(unit),
                        data: Box::new(Gauge { data_points }),
                    });
                }
            }
            MetricType::HISTOGRAM => {
                let data_points: Vec<_> = family
                    .get_metric()
                    .iter()
                    .map(|m| {
                        let h = m.get_histogram();
                        let (bounds, bucket_counts) =
                            cumulative_buckets_to_explicit(h.get_bucket(), h.get_sample_count());
                        HistogramDataPoint {
                            attributes: labels_to_attrs(m.get_label()),
                            start_time: series_start,
                            time: now,
                            count: h.get_sample_count(),
                            bounds,
                            bucket_counts,
                            min: None,
                            max: None,
                            sum: h.get_sample_sum(),
                            exemplars: vec![],
                        }
                    })
                    .collect();
                if !data_points.is_empty() {
                    scope.metrics.push(Metric {
                        name: Cow::Owned(name.to_string()),
                        description: Cow::Owned(description),
                        unit: Cow::Borrowed(unit),
                        data: Box::new(Histogram {
                            data_points,
                            temporality: Temporality::Cumulative,
                        }),
                    });
                }
            }
            MetricType::SUMMARY | MetricType::UNTYPED => {
                // Conduit does not emit these instrument types.
            }
        }
    }

    ResourceMetrics {
        resource: Resource::new(resource_attributes),
        scope_metrics: vec![scope],
    }
}

fn labels_to_attrs(labels: &[prometheus::proto::LabelPair]) -> Vec<KeyValue> {
    labels
        .iter()
        .map(|lp| KeyValue::new(lp.get_name().to_string(), lp.get_value().to_string()))
        .collect()
}

/// Derive a UCUM-ish OTel unit from a Prometheus metric name suffix.
fn unit_from_metric_name(name: &str) -> &'static str {
    const SUFFIXES: &[(&str, &str)] = &[
        ("_seconds", "s"),
        ("_milliseconds", "ms"),
        ("_bytes", "By"),
        ("_ratio", "1"),
        ("_ms", "ms"),
    ];
    for (suffix, unit) in SUFFIXES {
        if name.ends_with(suffix) {
            return unit;
        }
    }
    if name.ends_with("_total") || name.ends_with("_info") {
        return "1";
    }
    ""
}

/// Convert Prometheus cumulative histogram buckets (+ implicit +Inf via `sample_count`)
/// into OTel explicit bucket counts (`bucket_counts.len() == bounds.len() + 1`).
fn cumulative_buckets_to_explicit(
    buckets: &[prometheus::proto::Bucket],
    sample_count: u64,
) -> (Vec<f64>, Vec<u64>) {
    let mut bounds = Vec::new();
    let mut cumulatives = Vec::new();
    for b in buckets {
        let ub = b.get_upper_bound();
        if ub.is_finite() {
            bounds.push(ub);
            cumulatives.push(b.get_cumulative_count());
        }
    }

    let mut bucket_counts = Vec::with_capacity(bounds.len() + 1);
    let mut prev = 0u64;
    for &cum in &cumulatives {
        bucket_counts.push(cum.saturating_sub(prev));
        prev = cum;
    }
    bucket_counts.push(sample_count.saturating_sub(prev));
    (bounds, bucket_counts)
}

#[cfg(test)]
mod tests {
    use super::*;
    use prometheus::proto::{
        Bucket as ProtoBucket, Counter as ProtoCounter, Gauge as ProtoGauge,
        Histogram as ProtoHistogram, LabelPair, Metric as ProtoMetric, MetricFamily, MetricType,
    };

    fn series_start() -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000)
    }

    fn counter_family(name: &str, help: &str, labels: &[(&str, &str)], value: f64) -> MetricFamily {
        let mut family = MetricFamily::default();
        family.set_name(name.to_string());
        family.set_help(help.to_string());
        family.set_field_type(MetricType::COUNTER);
        let mut metric = ProtoMetric::default();
        metric.set_label(
            labels
                .iter()
                .map(|(k, v)| {
                    let mut lp = LabelPair::default();
                    lp.set_name((*k).to_string());
                    lp.set_value((*v).to_string());
                    lp
                })
                .collect::<Vec<_>>()
                .into(),
        );
        let mut c = ProtoCounter::default();
        c.set_value(value);
        metric.set_counter(c);
        family.set_metric(vec![metric].into());
        family
    }

    fn gauge_family(name: &str, help: &str, value: f64) -> MetricFamily {
        let mut family = MetricFamily::default();
        family.set_name(name.to_string());
        family.set_help(help.to_string());
        family.set_field_type(MetricType::GAUGE);
        let mut metric = ProtoMetric::default();
        let mut g = ProtoGauge::default();
        g.set_value(value);
        metric.set_gauge(g);
        family.set_metric(vec![metric].into());
        family
    }

    fn histogram_family(
        name: &str,
        help: &str,
        finite_buckets: &[(f64, u64)],
        sample_count: u64,
        sample_sum: f64,
    ) -> MetricFamily {
        let mut family = MetricFamily::default();
        family.set_name(name.to_string());
        family.set_help(help.to_string());
        family.set_field_type(MetricType::HISTOGRAM);
        let mut metric = ProtoMetric::default();
        let mut h = ProtoHistogram::default();
        h.set_sample_count(sample_count);
        h.set_sample_sum(sample_sum);
        h.set_bucket(
            finite_buckets
                .iter()
                .map(|(ub, cum)| {
                    let mut b = ProtoBucket::default();
                    b.set_upper_bound(*ub);
                    b.set_cumulative_count(*cum);
                    b
                })
                .collect::<Vec<_>>()
                .into(),
        );
        metric.set_histogram(h);
        family.set_metric(vec![metric].into());
        family
    }

    #[test]
    fn unit_from_metric_name_table() {
        assert_eq!(
            unit_from_metric_name("conduit_forward_duration_seconds"),
            "s"
        );
        assert_eq!(
            unit_from_metric_name("conduit_process_resident_bytes"),
            "By"
        );
        assert_eq!(
            unit_from_metric_name("conduit_backend_health_latency_ewma_ms"),
            "ms"
        );
        assert_eq!(unit_from_metric_name("conduit_queries_total"), "1");
        assert_eq!(unit_from_metric_name("conduit_build_info"), "1");
        assert_eq!(unit_from_metric_name("conduit_config_generation"), "");
    }

    #[test]
    fn cumulative_buckets_to_explicit_diffs_and_inf() {
        let mut b0 = ProtoBucket::default();
        b0.set_upper_bound(0.001);
        b0.set_cumulative_count(2);
        let mut b1 = ProtoBucket::default();
        b1.set_upper_bound(0.01);
        b1.set_cumulative_count(5);
        let mut b2 = ProtoBucket::default();
        b2.set_upper_bound(0.05);
        b2.set_cumulative_count(8);
        let (bounds, counts) = cumulative_buckets_to_explicit(&[b0, b1, b2], 10);
        assert_eq!(bounds, vec![0.001, 0.01, 0.05]);
        assert_eq!(counts, vec![2, 3, 3, 2]);
    }

    #[test]
    fn counter_maps_help_unit_start_time_and_groups_points() {
        let mut family = counter_family(
            "conduit_queries_total",
            "DNS queries received",
            &[("listener", "ln"), ("protocol", "udp")],
            3.0,
        );
        // Second label set on same family.
        let mut m2 = ProtoMetric::default();
        let mut lp = LabelPair::default();
        lp.set_name("listener".into());
        lp.set_value("other".into());
        let mut lp2 = LabelPair::default();
        lp2.set_name("protocol".into());
        lp2.set_value("tcp".into());
        m2.set_label(vec![lp, lp2].into());
        let mut c = ProtoCounter::default();
        c.set_value(7.0);
        m2.set_counter(c);
        let mut metrics: Vec<_> = family.take_metric().into();
        metrics.push(m2);
        family.set_metric(metrics.into());

        let rm = families_to_resource_metrics(&[family], vec![], series_start());
        assert_eq!(rm.scope_metrics[0].metrics.len(), 1);
        let m = &rm.scope_metrics[0].metrics[0];
        assert_eq!(m.name, "conduit_queries_total");
        assert_eq!(m.description, "DNS queries received");
        assert_eq!(m.unit, "1");
        let sum = m.data.as_any().downcast_ref::<Sum<u64>>().expect("sum");
        assert_eq!(sum.data_points.len(), 2);
        assert_eq!(sum.data_points[0].start_time, Some(series_start()));
        assert_eq!(sum.data_points[0].value, 3);
        assert_eq!(sum.data_points[1].value, 7);
    }

    #[test]
    fn gauge_maps_help_and_value() {
        let family = gauge_family("conduit_config_generation", "Config generation", 2.0);
        let rm = families_to_resource_metrics(&[family], vec![], series_start());
        let m = rm.scope_metrics[0]
            .metrics
            .iter()
            .find(|m| m.name == "conduit_config_generation")
            .unwrap();
        assert_eq!(m.description, "Config generation");
        assert_eq!(m.unit, "");
        let gauge = m.data.as_any().downcast_ref::<Gauge<f64>>().expect("gauge");
        assert_eq!(gauge.data_points[0].value, 2.0);
        assert!(gauge.data_points[0].start_time.is_none());
    }

    #[test]
    fn histogram_maps_family_name_sum_count_and_explicit_buckets() {
        let family = histogram_family(
            "conduit_forward_duration_seconds",
            "Forward RTT",
            &[(0.001, 2), (0.01, 5), (0.05, 8)],
            10,
            0.042,
        );
        let rm = families_to_resource_metrics(&[family], vec![], series_start());
        assert_eq!(rm.scope_metrics[0].metrics.len(), 1);
        let m = &rm.scope_metrics[0].metrics[0];
        assert_eq!(m.name, "conduit_forward_duration_seconds");
        assert_eq!(m.description, "Forward RTT");
        assert_eq!(m.unit, "s");
        let hist = m
            .data
            .as_any()
            .downcast_ref::<Histogram<f64>>()
            .expect("histogram");
        assert_eq!(hist.data_points.len(), 1);
        let dp = &hist.data_points[0];
        assert_eq!(dp.count, 10);
        assert!((dp.sum - 0.042).abs() < f64::EPSILON);
        assert_eq!(dp.bounds, vec![0.001, 0.01, 0.05]);
        assert_eq!(dp.bucket_counts, vec![2, 3, 3, 2]);
        assert_eq!(dp.start_time, series_start());
    }

    #[test]
    fn builtin_forward_duration_otlp_matches_prom_sum_and_count() {
        let reg = crate::builtin::BuiltinRegistry::new(true, crate::compile::BuiltinProfile::Full);
        reg.record_forward_duration("default", "127.0.0.1:5300", 0.001);
        reg.record_forward_duration("default", "127.0.0.1:5300", 0.02);
        reg.record_forward_duration("default", "127.0.0.1:5300", 0.2);
        let families = reg.gather();
        let prom_family = families
            .iter()
            .find(|f| f.get_name() == "conduit_forward_duration_seconds")
            .expect("prom histogram family");
        let prom_h = prom_family.get_metric()[0].get_histogram();

        let rm = families_to_resource_metrics(&families, vec![], series_start());
        let m = rm.scope_metrics[0]
            .metrics
            .iter()
            .find(|m| m.name == "conduit_forward_duration_seconds")
            .expect("otel histogram");
        let hist = m
            .data
            .as_any()
            .downcast_ref::<Histogram<f64>>()
            .expect("histogram");
        let dp = &hist.data_points[0];
        assert_eq!(dp.count, prom_h.get_sample_count());
        assert!((dp.sum - prom_h.get_sample_sum()).abs() < 1e-12);
        assert_eq!(dp.bucket_counts.iter().sum::<u64>(), dp.count);
        assert_eq!(m.unit, "s");
        assert_eq!(m.description, prom_family.get_help());
    }

    #[test]
    fn parity_matrix_core_4b_families() {
        let reg = crate::builtin::BuiltinRegistry::new(true, crate::compile::BuiltinProfile::Full);
        let addr: std::net::SocketAddr = "127.0.0.1:15353".parse().unwrap();
        reg.record_query("ln", "udp", Some(1), Some(1), &addr);
        reg.record_query_by_pool("default");
        reg.record_parse_rejected("wire_error");
        reg.record_query_dropped("ln", "udp", "request_rules", &addr);
        reg.record_response("ln", "udp", Some(0), &addr, Some("forward"));
        let families = reg.gather();

        let prom_names: std::collections::HashSet<_> =
            families.iter().map(|f| f.get_name().to_string()).collect();
        let rm = families_to_resource_metrics(&families, vec![], series_start());
        let otel_names: std::collections::HashSet<_> = rm
            .scope_metrics
            .iter()
            .flat_map(|s| s.metrics.iter().map(|m| m.name.to_string()))
            .collect();

        for family in [
            "conduit_queries_total",
            "conduit_queries_by_pool_total",
            "conduit_queries_dropped_total",
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
