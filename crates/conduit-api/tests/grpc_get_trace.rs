mod support;

use conduit_config::load_yaml;
use conduit_core::{
    orchestrator::{Orchestrator, RunOutcome},
    phase::Phase,
    pipeline::{PipelineStage, StageOutcome},
    snapshot::RuntimeSnapshot,
    transaction::ClientProtocol,
    SystemClock, Transaction,
};
use conduit_proto::control::conduit_control_client::ConduitControlClient;
use conduit_proto::control::{GetTraceRequest, HealthRequest};
use hickory_proto::op::{Message, Query, ResponseCode};
use hickory_proto::rr::{Name, RecordType};
use hickory_proto::serialize::binary::{BinEncodable, BinEncoder};
use std::net::SocketAddr;
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

struct MockForwardStage;

impl PipelineStage for MockForwardStage {
    fn name(&self) -> &'static str {
        "mock_forward"
    }

    fn handle(&self, txn: &mut Transaction, _snapshot: &Arc<RuntimeSnapshot>) -> StageOutcome {
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

#[tokio::test]
async fn get_trace_returns_events_after_traced_query() {
    let yaml = include_str!("../../../tests/fixtures/config/with-tracing-selectors.yaml");
    let file_cfg = load_yaml(yaml).expect("parse");
    let (snapshots, effective, configurator, tracing, base_dir) = support::control_setup(
        file_cfg,
        support::workspace_fixture("tests/fixtures/config/with-tracing-selectors.yaml"),
        Some(support::workspace_fixture("tests/fixtures/config")),
    );

    let addr: SocketAddr = "127.0.0.1:0".parse().expect("parse");
    let local_addr = conduit_api::serve_on_listener(
        addr,
        snapshots.clone(),
        effective,
        configurator,
        tracing.clone(),
        base_dir,
    )
    .await
    .expect("start server");

    let mut orch = Orchestrator::with_default_stages();
    orch.tracing = Some(tracing.clone());
    orch.registry
        .register(Phase::Forward, Arc::new(MockForwardStage));
    orch.registry
        .register(Phase::WaitResponse, Arc::new(PassthroughWait));

    let snap = snapshots.load();
    let txn_id = 42_u64;
    let mut txn = Transaction::new(
        txn_id,
        "127.0.0.1:15353".parse().unwrap(),
        ClientProtocol::Udp,
    )
    .with_query_wire(sample_query());
    assert!(matches!(
        orch.run(&mut txn, &snap, &SystemClock, None),
        RunOutcome::Response(_)
    ));

    let endpoint = format!("http://{local_addr}");
    let mut client = ConduitControlClient::connect(endpoint)
        .await
        .expect("connect");
    let _ = client
        .health(HealthRequest {})
        .await
        .expect("health")
        .into_inner();

    let trace = client
        .get_trace(GetTraceRequest {
            txn_id: txn_id.to_string(),
        })
        .await
        .expect("get_trace")
        .into_inner();

    assert!(trace.found, "expected trace for txn {txn_id}");
    assert!(
        !trace.events.is_empty(),
        "expected phase events, got {:?}",
        trace.events
    );
    assert!(
        trace
            .events
            .iter()
            .any(|e| e.phase == "route" || e.phase == "forward"),
        "events: {:?}",
        trace.events
    );
}

#[tokio::test]
async fn get_trace_unknown_id_not_found() {
    let yaml = include_str!("../../../tests/fixtures/config/with-tracing-selectors.yaml");
    let file_cfg = load_yaml(yaml).expect("parse");
    let (snapshots, effective, configurator, tracing, base_dir) = support::control_setup(
        file_cfg,
        support::workspace_fixture("tests/fixtures/config/with-tracing-selectors.yaml"),
        Some(support::workspace_fixture("tests/fixtures/config")),
    );

    let addr: SocketAddr = "127.0.0.1:0".parse().expect("parse");
    let local_addr = conduit_api::serve_on_listener(
        addr,
        snapshots,
        effective,
        configurator,
        tracing.clone(),
        base_dir,
    )
    .await
    .expect("start server");

    let endpoint = format!("http://{local_addr}");
    let mut client = ConduitControlClient::connect(endpoint)
        .await
        .expect("connect");

    let trace = client
        .get_trace(GetTraceRequest {
            txn_id: "999999".into(),
        })
        .await
        .expect("get_trace")
        .into_inner();

    assert!(!trace.found);
    assert!(trace.events.is_empty());
}
