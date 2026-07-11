//! Gather Prometheus metric families and render text for scrape / OTLP conversion.

use crate::MetricsHub;
use conduit_events::SinkMetricsSnapshot;
use prometheus::{Encoder, IntCounterVec, Opts, Registry, TextEncoder};

/// All metric families (built-in, user, event-sink) for scrape or OTLP conversion.
pub fn gather_prometheus_families(
    hub: &MetricsHub,
    event_sinks: &[SinkMetricsSnapshot],
) -> Vec<prometheus::proto::MetricFamily> {
    let mut families = hub.builtin.gather();
    families.extend(hub.user.gather());
    families.extend(event_sink_families(event_sinks));
    families
}

pub fn render_prometheus(hub: &MetricsHub, event_sinks: &[SinkMetricsSnapshot]) -> String {
    encode_families(gather_prometheus_families(hub, event_sinks))
}

fn encode_families(families: Vec<prometheus::proto::MetricFamily>) -> String {
    if families.is_empty() {
        return String::new();
    }
    let encoder = TextEncoder::new();
    let mut buf = Vec::new();
    encoder.encode(&families, &mut buf).expect("encode");
    String::from_utf8(buf).expect("utf8")
}

fn event_sink_families(snapshots: &[SinkMetricsSnapshot]) -> Vec<prometheus::proto::MetricFamily> {
    if snapshots.is_empty() {
        return Vec::new();
    }
    let registry = Registry::new();
    let enqueued_query = IntCounterVec::new(
        Opts::new(
            "conduit_events_enqueued_query_total",
            "DNS export events enqueued (query)",
        ),
        &["sink"],
    )
    .expect("metric");
    let enqueued_response = IntCounterVec::new(
        Opts::new(
            "conduit_events_enqueued_response_total",
            "DNS export events enqueued (response)",
        ),
        &["sink"],
    )
    .expect("metric");
    let queue_dropped = IntCounterVec::new(
        Opts::new(
            "conduit_events_queue_dropped_total",
            "Event sink queue drops",
        ),
        &["sink"],
    )
    .expect("metric");
    let delivered = IntCounterVec::new(
        Opts::new(
            "conduit_events_delivered_total",
            "Event sink frames delivered",
        ),
        &["sink"],
    )
    .expect("metric");
    registry
        .register(Box::new(enqueued_query.clone()))
        .expect("register");
    registry
        .register(Box::new(enqueued_response.clone()))
        .expect("register");
    registry
        .register(Box::new(queue_dropped.clone()))
        .expect("register");
    registry
        .register(Box::new(delivered.clone()))
        .expect("register");
    for s in snapshots {
        let sink = s.name.as_str();
        enqueued_query
            .with_label_values(&[sink])
            .inc_by(s.enqueued_query);
        enqueued_response
            .with_label_values(&[sink])
            .inc_by(s.enqueued_response);
        queue_dropped
            .with_label_values(&[sink])
            .inc_by(s.queue_dropped);
        delivered.with_label_values(&[sink]).inc_by(s.delivered);
    }
    registry.gather()
}
