//! Event export collect/emit axis: scrape/OTLP omit `conduit_events_*` when
//! emit is false; EventHub snapshot API still returns counters when collect is true.

use conduit_config::{load_yaml, validate};
use conduit_core::snapshot::RuntimeSnapshot;
use conduit_events::{EventHub, SinkMetricsSnapshot, TxnExtraSource, TxnView};
use conduit_metrics::{gather_prometheus_families, render_prometheus, MetricsHub};
use std::net::{IpAddr, Ipv4Addr, SocketAddr};

fn sample_sink_snapshot() -> SinkMetricsSnapshot {
    SinkMetricsSnapshot {
        name: "prod-tap".into(),
        enqueued_query: 3,
        enqueued_response: 0,
        enqueued_retry: 0,
        queue_dropped: 0,
        delivered: 0,
        write_failed: 0,
        encode_failed: 0,
        connect_attempts: 0,
        connected: 0,
    }
}

fn config_yaml(emit: bool, with_sink: bool) -> String {
    let sinks = if with_sink {
        r#"
  sinks:
    - type: dnstap
      name: prod-tap
      destinations:
        - "unix:/tmp/conduit-event-export-test.sock"
      emit:
        - query
"#
    } else {
        ""
    };
    format!(
        r#"
schema_version: 1
listeners:
  threads: 1
  reuse_port: true
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
events:
  queue_depth: 4096
  drop_policy: drop_oldest
{sinks}
rhai:
  max_operations: 10000
  max_call_depth: 32
pools:
  - name: default
    backends:
      - address: "127.0.0.1:15300"
        weight: 100
control:
  listen_address: "127.0.0.1:5199"
metrics:
  enabled: true
  base: standard
  event_export:
    collect: true
    emit: {emit}
  prometheus:
    listen_address: "127.0.0.1:19090"
    path: /metrics
"#
    )
}

fn hub_with_event_export(emit: bool) -> MetricsHub {
    let cfg = load_yaml(&config_yaml(emit, false)).expect("load yaml");
    assert!(validate(&cfg).ok, "validate: {:?}", validate(&cfg).errors);
    MetricsHub::from_config(&cfg)
}

fn sample_view() -> TxnView<'static> {
    TxnView {
        txn_id: 1,
        global_query_index: 1,
        client_addr: SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 53),
        protocol_udp: true,
        qname: Some("example."),
        qtype: Some(1),
        rcode: None,
        qclass: Some(1),
        opcode: None,
        edns_option_codes: &[],
        qtype_label: Some("A".into()),
        query_wire: &[0u8; 12],
        response_wire: None,
        attempt_count: 1,
        answer_source: None,
        cache_instance: None,
        extra: TxnExtraSource::default(),
    }
}

#[test]
fn event_export_emit_true_includes_conduit_events_in_scrape() {
    let hub = hub_with_event_export(true);
    assert!(hub.compiled().plan.event_export_collect);
    assert!(hub.compiled().plan.event_export_emit);

    let snaps = vec![sample_sink_snapshot()];
    let body = render_prometheus(&hub, &snaps);
    assert!(
        body.contains("conduit_events_enqueued_query_total"),
        "expected events series when emit true; body:\n{body}"
    );
    assert!(
        body.contains(r#"sink="prod-tap""#),
        "expected sink label; body:\n{body}"
    );
}

#[test]
fn event_export_emit_false_omits_conduit_events_preserves_snapshot_api() {
    let cfg = load_yaml(&config_yaml(false, true)).expect("load yaml");
    assert!(validate(&cfg).ok, "validate: {:?}", validate(&cfg).errors);
    let hub = MetricsHub::from_config(&cfg);
    assert!(hub.compiled().plan.event_export_collect);
    assert!(!hub.compiled().plan.event_export_emit);

    let snap = RuntimeSnapshot::from_config(cfg);
    let obs = EventHub::from_compiled(&snap.events);
    obs.try_enqueue_query(sample_view(), &snap.events, |_| true);

    let hub_snaps = obs.sink_metrics_snapshot();
    assert_eq!(hub_snaps.len(), 1);
    assert_eq!(hub_snaps[0].name, "prod-tap");
    assert!(
        hub_snaps[0].enqueued_query >= 1,
        "collect true must keep EventHub snapshot counters; got {:?}",
        hub_snaps[0]
    );

    let body = render_prometheus(&hub, &hub_snaps);
    assert!(
        !body.contains("conduit_events_"),
        "emit false must omit conduit_events_*; body:\n{body}"
    );

    let families = gather_prometheus_families(&hub, &hub_snaps);
    assert!(
        families
            .iter()
            .all(|f| !f.get_name().starts_with("conduit_events_")),
        "gather must also omit events families"
    );

    obs.shutdown();
}

#[test]
fn event_export_emit_false_otlp_parity_omits_events_families() {
    // OTLP push converts the same gather_prometheus_families output; assert the
    // shared gather path filters events when emit is false (Prom/OTLP parity).
    let hub = hub_with_event_export(false);
    let snaps = vec![sample_sink_snapshot()];
    let families = gather_prometheus_families(&hub, &snaps);
    assert!(
        !families
            .iter()
            .any(|f| f.get_name().starts_with("conduit_events_")),
        "OTLP-shared gather must omit conduit_events_* when emit false"
    );

    let hub_on = hub_with_event_export(true);
    let families_on = gather_prometheus_families(&hub_on, &snaps);
    assert!(
        families_on
            .iter()
            .any(|f| f.get_name() == "conduit_events_enqueued_query_total"),
        "emit true must include events family for OTLP parity"
    );
}
