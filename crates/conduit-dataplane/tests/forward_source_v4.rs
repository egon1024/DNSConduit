//! Rhai set_source_v4 on upstream forward egress.

use conduit_config::load_yaml;
use conduit_core::clock::SystemClock;
use conduit_core::orchestrator::{Orchestrator, RunOutcome};
use conduit_core::phase::Phase;
use conduit_core::pipeline::{PipelineStage, StageOutcome};
use conduit_core::snapshot::RuntimeSnapshot;
use conduit_core::transaction::{ClientProtocol, Transaction};
use conduit_dataplane::forward::{TxnTable, UdpForwardStage};
use hickory_proto::op::{Message, Query, ResponseCode};
use hickory_proto::rr::{Name, RecordType};
use hickory_proto::serialize::binary::{BinEncodable, BinEncoder};
use std::net::{Ipv4Addr, SocketAddr, UdpSocket};
use std::path::PathBuf;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

fn sample_query() -> Vec<u8> {
    let name = Name::from_utf8("test.example.").unwrap();
    let query = Query::query(name, RecordType::A);
    let mut msg = Message::new();
    let mut header = *msg.header();
    header.set_id(0xabcd);
    msg.set_header(header);
    msg.add_query(query);
    let mut out = Vec::new();
    let mut enc = BinEncoder::new(&mut out);
    msg.emit(&mut enc).unwrap();
    out
}

struct PassthroughWait;

impl PipelineStage for PassthroughWait {
    fn name(&self) -> &'static str {
        "wait_response"
    }

    fn handle(&self, _txn: &mut Transaction, _snapshot: &Arc<RuntimeSnapshot>) -> StageOutcome {
        StageOutcome::Continue(Phase::ResponseRules)
    }
}

fn orchestrator_with_forward(snap: &RuntimeSnapshot, table: Arc<TxnTable>) -> Orchestrator {
    let forward = UdpForwardStage::new(
        table,
        &snap.forward,
        &snap.egress_bind_addresses_v4(),
        &snap.egress_bind_addresses_v6(),
        snap.forward.timeout_ms,
        None,
    )
    .expect("forward egress");
    let mut orch = Orchestrator::with_default_stages();
    orch.registry.register(Phase::Forward, Arc::new(forward));
    orch.registry
        .register(Phase::WaitResponse, Arc::new(PassthroughWait));
    orch
}

#[test]
fn rhai_set_source_v4_on_upstream_egress() {
    let peer_ip = Arc::new(Mutex::new(None));
    let peer_ip_c = peer_ip.clone();
    let (port_tx, port_rx) = mpsc::channel();

    let server = thread::spawn(move || {
        let sock = UdpSocket::bind("127.0.0.1:0").unwrap();
        port_tx.send(sock.local_addr().unwrap().port()).unwrap();
        let mut buf = [0u8; 4096];
        sock.set_read_timeout(Some(Duration::from_millis(2000)))
            .unwrap();
        let (_, peer) = sock.recv_from(&mut buf).unwrap();
        *peer_ip_c.lock().unwrap() = Some(peer.ip());

        let req = Message::from_vec(&buf).unwrap();
        let mut resp = Message::new();
        resp.set_id(req.id());
        resp.set_response_code(ResponseCode::NoError);
        resp.add_query(req.queries()[0].clone());
        let mut out = Vec::new();
        let mut enc = BinEncoder::new(&mut out);
        resp.emit(&mut enc).unwrap();
        sock.send_to(&out, peer).unwrap();
    });

    let backend_port = port_rx.recv().unwrap();
    let base = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/config");
    let mut cfg = load_yaml(include_str!(
        "../../../tests/fixtures/config/with-rhai-set-source-v4.yaml"
    ))
    .unwrap();
    cfg.pools[0].backends[0].address = format!("127.0.0.1:{backend_port}");

    let snap = RuntimeSnapshot::from_config_with_base(cfg, Some(&base));
    let table = Arc::new(TxnTable::new(64, 50));
    let orch = orchestrator_with_forward(&snap, table);

    let client: SocketAddr = "127.0.0.1:15353".parse().unwrap();
    let mut txn = Transaction::new(42, client, ClientProtocol::Udp).with_query_wire(sample_query());
    let outcome = orch.run(&mut txn, &Arc::new(snap), &SystemClock, None);
    assert!(matches!(outcome, RunOutcome::Response(_)));

    server.join().unwrap();
    let ip = peer_ip.lock().unwrap().expect("peer ip");
    assert_eq!(ip, Ipv4Addr::LOCALHOST);
}
