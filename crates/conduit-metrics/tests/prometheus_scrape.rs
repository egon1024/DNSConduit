use conduit_config::{load_yaml, validate};
use conduit_core::{
    orchestrator::{Orchestrator, RunOutcome},
    phase::Phase,
    pipeline::{PipelineStage, StageOutcome},
    snapshot::RuntimeSnapshot,
    transaction::ClientProtocol,
    SystemClock, Transaction,
};
use conduit_events::EventHub;
use conduit_metrics::{render_prometheus, MetricsHub};
use hickory_proto::op::{Message, Query, ResponseCode};
use hickory_proto::rr::{Name, RecordType};
use hickory_proto::serialize::binary::{BinEncodable, BinEncoder};
use std::path::PathBuf;
use std::sync::Arc;

fn fixtures_config_base() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/config")
}

fn sample_query() -> Vec<u8> {
    let name = Name::from_utf8("test.example.").unwrap();
    let query = Query::query(name, RecordType::A);
    let mut msg = Message::new();
    msg.add_query(query);
    let mut buf = Vec::new();
    let mut encoder = BinEncoder::new(&mut buf);
    msg.emit(&mut encoder).unwrap();
    buf
}

fn query_for(name: &str) -> Vec<u8> {
    let name = Name::from_utf8(name).unwrap();
    let query = Query::query(name, RecordType::A);
    let mut msg = Message::new();
    msg.add_query(query);
    let mut buf = Vec::new();
    let mut encoder = BinEncoder::new(&mut buf);
    msg.emit(&mut encoder).unwrap();
    buf
}

struct MockForwardStage {
    metrics: Option<Arc<MetricsHub>>,
}

impl PipelineStage for MockForwardStage {
    fn name(&self) -> &'static str {
        "mock_forward"
    }

    fn handle(&self, txn: &mut Transaction, _snapshot: &Arc<RuntimeSnapshot>) -> StageOutcome {
        if let Some(hub) = self.metrics.as_ref() {
            if hub.metrics_enabled() {
                let pool = txn.selected_pool.as_deref().unwrap_or("default");
                let backend = txn
                    .selected_backend
                    .map(|a| a.to_string())
                    .unwrap_or_else(|| "127.0.0.1:5300".into());
                hub.builtin
                    .record_forward_attempt(pool, &backend, "success");
                hub.builtin.record_forward_duration(pool, &backend, 0.001);
            }
        }
        let mut msg = Message::new();
        msg.set_id(txn.dns_id);
        msg.set_response_code(ResponseCode::NoError);
        let mut buf = Vec::new();
        let mut encoder = BinEncoder::new(&mut buf);
        msg.emit(&mut encoder).unwrap();
        txn.response_wire = Some(buf);
        txn.set_rcode(0);
        StageOutcome::Continue(Phase::WaitResponse)
    }
}

struct PassthroughWait;

impl PipelineStage for PassthroughWait {
    fn name(&self) -> &'static str {
        "wait"
    }

    fn handle(&self, _txn: &mut Transaction, _snapshot: &Arc<RuntimeSnapshot>) -> StageOutcome {
        StageOutcome::Continue(Phase::ResponseRules)
    }
}

fn orchestrator_with_mock_forward(hub: Arc<MetricsHub>) -> Orchestrator {
    let mut orch = Orchestrator::with_default_stages();
    orch.metrics = Some(hub.clone());
    orch.registry.register(
        Phase::RequestRules,
        Arc::new(conduit_core::stages::RequestRulesStage {
            metrics: Some(hub.clone()),
        }),
    );
    orch.registry.register(
        Phase::Forward,
        Arc::new(MockForwardStage { metrics: Some(hub) }),
    );
    orch.registry
        .register(Phase::WaitResponse, Arc::new(PassthroughWait));
    orch
}

#[test]
fn prometheus_text_includes_conduit_queries_after_traffic() {
    let yaml = include_str!("../../../tests/fixtures/config/with-metrics-prometheus.yaml");
    let cfg = load_yaml(yaml).unwrap();
    assert!(validate(&cfg).ok);
    let hub = Arc::new(MetricsHub::from_config(&cfg));
    assert!(hub.metrics_enabled());

    let snap = Arc::new(RuntimeSnapshot::from_config(cfg));
    let obs = EventHub::from_compiled(&snap.events);
    let orch = orchestrator_with_mock_forward(hub.clone());

    let mut txn = Transaction::new(1, "127.0.0.1:15353".parse().unwrap(), ClientProtocol::Udp)
        .with_listener_label("127.0.0.1:15353")
        .with_query_wire(sample_query());
    let _ = orch.run(&mut txn, &snap, &SystemClock, None);

    let body = render_prometheus(hub.as_ref(), &obs.sink_metrics_snapshot());
    assert!(body.contains("conduit_queries_total"));
    assert!(body.contains(r#"listener="127.0.0.1:15353""#));
}

#[test]
fn forward_metrics_recorded_on_mock_upstream() {
    let yaml = include_str!("../../../tests/fixtures/config/with-metrics-prometheus.yaml");
    let cfg = load_yaml(yaml).unwrap();
    let hub = Arc::new(MetricsHub::from_config(&cfg));
    let snap = Arc::new(RuntimeSnapshot::from_config(cfg));
    let obs = EventHub::from_compiled(&snap.events);
    let orch = orchestrator_with_mock_forward(hub.clone());

    let mut txn = Transaction::new(2, "127.0.0.1:15353".parse().unwrap(), ClientProtocol::Udp)
        .with_query_wire(sample_query());
    assert!(matches!(
        orch.run(&mut txn, &snap, &SystemClock, None),
        RunOutcome::Response(_)
    ));

    let body = render_prometheus(hub.as_ref(), &obs.sink_metrics_snapshot());
    assert!(
        body.contains("conduit_forward_attempts_total"),
        "body:\n{body}"
    );
    assert!(
        body.contains("conduit_forward_duration_seconds"),
        "body:\n{body}"
    );
}

fn counter_value(families: &[prometheus::proto::MetricFamily], name: &str) -> u64 {
    families
        .iter()
        .find(|f| f.get_name() == name)
        .and_then(|f| f.get_metric().first())
        .map(|m| m.get_counter().get_value() as u64)
        .unwrap_or(0)
}

#[test]
fn metrics_disabled_leaves_builtin_counters_at_zero() {
    let yaml = include_str!("../../../tests/fixtures/config/metrics-disabled.yaml");
    let cfg = load_yaml(yaml).unwrap();
    let hub = Arc::new(MetricsHub::from_config(&cfg));
    assert!(!hub.metrics_enabled());

    let snap = Arc::new(RuntimeSnapshot::from_config(cfg));
    let obs = EventHub::from_compiled(&snap.events);
    let orch = orchestrator_with_mock_forward(hub.clone());

    let before = counter_value(&hub.builtin.gather(), "conduit_queries_total");
    let mut txn = Transaction::new(3, "127.0.0.1:15353".parse().unwrap(), ClientProtocol::Udp)
        .with_query_wire(sample_query());
    let _ = orch.run(&mut txn, &snap, &SystemClock, None);
    let after = counter_value(&hub.builtin.gather(), "conduit_queries_total");

    assert_eq!(before, 0);
    assert_eq!(after, 0);
    let _body = render_prometheus(hub.as_ref(), &obs.sink_metrics_snapshot());
}

#[test]
fn parse_rejected_metric_after_drop() {
    let yaml = include_str!("../../../tests/fixtures/config/with-metrics-prometheus.yaml");
    let cfg = load_yaml(yaml).unwrap();
    let hub = Arc::new(MetricsHub::from_config(&cfg));
    let snap = Arc::new(RuntimeSnapshot::from_config(cfg));
    let orch = Orchestrator::with_default_stages();
    let mut orch = orch;
    orch.metrics = Some(hub.clone());

    let mut txn = Transaction::new(20, "127.0.0.1:15353".parse().unwrap(), ClientProtocol::Udp)
        .with_listener_label("127.0.0.1:15353");
    let _ = orch.run(&mut txn, &snap, &SystemClock, None);

    let body = render_prometheus(hub.as_ref(), &[]);
    assert!(
        body.contains("conduit_parse_rejected_total"),
        "body:\n{body}"
    );
    assert!(body.contains(r#"reason="empty""#), "body:\n{body}");
}

#[test]
fn minimal_profile_includes_coarse_responses_total() {
    let yaml = include_str!("../../../tests/fixtures/config/with-metrics-prometheus-minimal.yaml");
    let cfg = load_yaml(yaml).unwrap();
    let hub = Arc::new(MetricsHub::from_config(&cfg));
    assert_eq!(hub.builtin.profile(), conduit_metrics::BuiltinProfile::Minimal);
    let snap = Arc::new(RuntimeSnapshot::from_config(cfg));
    let orch = orchestrator_with_mock_forward(hub.clone());

    let mut txn = Transaction::new(22, "127.0.0.1:15353".parse().unwrap(), ClientProtocol::Udp)
        .with_listener_label("127.0.0.1:15353")
        .with_query_wire(sample_query());
    let _ = orch.run(&mut txn, &snap, &SystemClock, None);

    let body = render_prometheus(hub.as_ref(), &[]);
    assert!(body.contains("conduit_responses_total"), "body:\n{body}");
    assert!(body.contains(r#"rcode="NOERROR""#), "body:\n{body}");
    assert!(
        !body.contains("conduit_parse_rejected_total"),
        "parse_rejected remains full-only, body:\n{body}"
    );
}

#[test]
fn full_profile_includes_qtype_on_queries() {
    let yaml = include_str!("../../../tests/fixtures/config/with-metrics-prometheus.yaml");
    let cfg = load_yaml(yaml).unwrap();
    let hub = Arc::new(MetricsHub::from_config(&cfg));
    let snap = Arc::new(RuntimeSnapshot::from_config(cfg));
    let orch = orchestrator_with_mock_forward(hub.clone());

    let mut txn = Transaction::new(21, "127.0.0.1:15353".parse().unwrap(), ClientProtocol::Udp)
        .with_listener_label("127.0.0.1:15353")
        .with_query_wire(sample_query());
    let _ = orch.run(&mut txn, &snap, &SystemClock, None);

    let body = render_prometheus(hub.as_ref(), &[]);
    assert!(body.contains(r#"qtype="A""#), "body:\n{body}");
}

#[test]
fn rhai_user_metric_accumulates_across_queries() {
    let yaml = include_str!("../../../tests/fixtures/config/with-rhai-block-hits.yaml");
    let cfg = load_yaml(yaml).unwrap();
    let base = fixtures_config_base();
    let snap = Arc::new(RuntimeSnapshot::try_from_config_with_base(cfg, Some(&base)).unwrap());
    let hub = Arc::new(MetricsHub::from_config(&snap.config));
    let obs = EventHub::from_compiled(&snap.events);
    let orch = orchestrator_with_mock_forward(hub.clone());

    for id in 10..12 {
        let mut txn = Transaction::new(id, "127.0.0.1:15353".parse().unwrap(), ClientProtocol::Udp)
            .with_query_wire(query_for("eu.example."));
        let _ = orch.run(&mut txn, &snap, &SystemClock, None);
    }

    let body = render_prometheus(hub.as_ref(), &obs.sink_metrics_snapshot());
    assert!(body.contains("conduit_user_block_hits"), "body:\n{body}");
    assert!(
        body.contains("conduit_user_block_hits 2"),
        "expected cumulative user counter 2, body:\n{body}"
    );
}
