use conduit_config::load_yaml;
use conduit_core::{
    orchestrator::Orchestrator, snapshot::RuntimeSnapshot, transaction::ClientProtocol,
    SystemClock, Transaction,
};
use hickory_proto::op::{Message, Query};
use hickory_proto::rr::{Name, RecordType};
use hickory_proto::serialize::binary::{BinEncodable, BinEncoder};
use std::sync::Arc;

#[test]
fn tracing_disabled_does_not_allocate_trace_log() {
    let yaml = include_str!("../../../tests/fixtures/config/minimal.yaml");
    let cfg = load_yaml(yaml).unwrap();
    let snap = Arc::new(RuntimeSnapshot::from_config(cfg));
    let orch = Orchestrator::with_default_stages();
    let mut txn = Transaction::new(1, "127.0.0.1:5353".parse().unwrap(), ClientProtocol::Udp);
    let name = Name::from_utf8("x.example.").unwrap();
    let query = Query::query(name, RecordType::A);
    let mut msg = Message::new();
    msg.add_query(query);
    let mut buf = Vec::new();
    let mut encoder = BinEncoder::new(&mut buf);
    msg.emit(&mut encoder).unwrap();
    txn.query_wire = buf;
    let _ = orch.run(&mut txn, &snap, &SystemClock, None);
    assert!(txn.trace_log.is_none());
}
