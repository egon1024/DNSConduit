use conduit_config::{load_yaml, validate};
use conduit_core::{
    orchestrator::Orchestrator, snapshot::RuntimeSnapshot, transaction::ClientProtocol,
    SystemClock, Transaction,
};
use conduit_metrics::{render_prometheus, MetricsHub};
use conduit_observation::ObservationHub;
use hickory_proto::op::{Message, Query};
use hickory_proto::rr::{Name, RecordType};
use hickory_proto::serialize::binary::{BinEncodable, BinEncoder};
use std::sync::Arc;

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

#[test]
fn prometheus_text_includes_conduit_queries_after_traffic() {
    let yaml = include_str!("../../../tests/fixtures/config/with-metrics-prometheus.yaml");
    let cfg = load_yaml(yaml).unwrap();
    assert!(validate(&cfg).ok);
    let hub = Arc::new(MetricsHub::from_config(&cfg));
    assert!(hub.metrics_enabled());

    let snap = Arc::new(RuntimeSnapshot::from_config(cfg));
    let obs = ObservationHub::from_compiled(&snap.observation);
    let mut orch = Orchestrator::with_default_stages();
    orch.metrics = Some(hub.clone());

    let mut txn = Transaction::new(1, "127.0.0.1:5353".parse().unwrap(), ClientProtocol::Udp)
        .with_query_wire(sample_query());
    let _ = orch.run(&mut txn, &snap, &SystemClock, None);

    let body = render_prometheus(hub.as_ref(), &obs.sink_metrics_snapshot());
    assert!(body.contains("conduit_queries_total"));
}
