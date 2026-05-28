//! Render Prometheus text from all metric sources.

use crate::builtin::encode_builtin;
use crate::MetricsHub;
use conduit_observation::SinkMetricsSnapshot;
use prometheus::{Encoder, IntCounterVec, Opts, Registry, TextEncoder};

pub fn render_prometheus(hub: &MetricsHub, observation: &[SinkMetricsSnapshot]) -> String {
    [
        encode_builtin(hub.builtin.gather()),
        encode_families(hub.user.gather()),
        encode_observation(observation),
    ]
    .join("\n")
}

fn encode_families(families: Vec<prometheus::proto::MetricFamily>) -> String {
    let encoder = TextEncoder::new();
    let mut buf = Vec::new();
    encoder.encode(&families, &mut buf).expect("encode");
    String::from_utf8(buf).expect("utf8")
}

fn encode_observation(snapshots: &[SinkMetricsSnapshot]) -> String {
    if snapshots.is_empty() {
        return String::new();
    }
    let registry = Registry::new();
    let enqueued_query = IntCounterVec::new(
        Opts::new(
            "conduit_observation_enqueued_query_total",
            "Observation events enqueued (query)",
        ),
        &["sink"],
    )
    .expect("metric");
    let enqueued_response = IntCounterVec::new(
        Opts::new(
            "conduit_observation_enqueued_response_total",
            "Observation events enqueued (response)",
        ),
        &["sink"],
    )
    .expect("metric");
    let queue_dropped = IntCounterVec::new(
        Opts::new(
            "conduit_observation_queue_dropped_total",
            "Observation queue drops",
        ),
        &["sink"],
    )
    .expect("metric");
    let delivered = IntCounterVec::new(
        Opts::new(
            "conduit_observation_delivered_total",
            "Observation frames delivered",
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
    encode_families(registry.gather())
}
