//! Regression tests for DNS label compression on query/response wire paths.

mod wire_fixtures;

use std::net::SocketAddr;
use std::sync::Arc;

use conduit_config::load_yaml;
use conduit_core::parse_reject::ParseRejectReason;
use conduit_core::phase::Phase;
use conduit_core::pipeline::{PipelineStage, StageOutcome};
use conduit_core::snapshot::RuntimeSnapshot;
use conduit_core::stages::parse::ParseStage;
use conduit_core::stages::send::build_error_response;
use conduit_core::transaction::{ClientProtocol, Transaction};
use hickory_proto::op::{Message, MessageType, ResponseCode};
use wire_fixtures::{
    hickory_compressed_response_bytes, query_with_forward_name_pointer,
    query_with_recursive_name_pointer, response_with_compressed_answer_name, valid_a_query_wire,
    wire_contains_compression_pointer,
};

fn snapshot() -> Arc<RuntimeSnapshot> {
    let yaml = include_str!("../../../../tests/fixtures/config/minimal.yaml");
    let cfg = load_yaml(yaml).unwrap();
    Arc::new(RuntimeSnapshot::from_config(cfg))
}

fn txn_with_wire(wire: Vec<u8>) -> Transaction {
    Transaction::new(
        1,
        "127.0.0.1:15353".parse::<SocketAddr>().unwrap(),
        ClientProtocol::Udp,
    )
    .with_query_wire(wire)
}

#[test]
fn parse_rejects_forward_name_compression_pointer() {
    let wire = query_with_forward_name_pointer();
    let mut txn = txn_with_wire(wire);
    assert_eq!(ParseStage.handle(&mut txn, &snapshot()), StageOutcome::Drop);
    assert_eq!(txn.parse_reject_reason, Some(ParseRejectReason::WireError));
}

#[test]
fn parse_rejects_recursive_name_compression_pointer() {
    let wire = query_with_recursive_name_pointer();
    let mut txn = txn_with_wire(wire);
    assert_eq!(ParseStage.handle(&mut txn, &snapshot()), StageOutcome::Drop);
    assert_eq!(txn.parse_reject_reason, Some(ParseRejectReason::WireError));
}

#[test]
fn parse_accepts_valid_uncompressed_query() {
    let wire = valid_a_query_wire();
    assert!(!wire_contains_compression_pointer(&wire));
    let mut txn = txn_with_wire(wire);
    assert_eq!(
        ParseStage.handle(&mut txn, &snapshot()),
        StageOutcome::Continue(Phase::RequestRules)
    );
    assert_eq!(txn.qname.as_deref(), Some("www.example.com."));
}

#[test]
fn response_with_compressed_answer_parses_qname_and_rcode() {
    let wire = response_with_compressed_answer_name();
    let msg = Message::from_vec(&wire).expect("compressed response must parse");
    assert_eq!(msg.header().message_type(), MessageType::Response);
    assert_eq!(msg.response_code(), ResponseCode::NoError);
    assert_eq!(
        msg.queries().first().unwrap().name().to_utf8(),
        "www.example.com."
    );
    assert_eq!(msg.answers().len(), 1);
}

#[test]
fn hickory_compressed_response_fixture_round_trips() {
    let wire = hickory_compressed_response_bytes();
    let msg = Message::from_vec(&wire).expect("fixed fixture must parse");
    assert_eq!(msg.id(), 4096);
    assert_eq!(
        msg.queries().first().unwrap().name().to_utf8(),
        "www.example.com."
    );
    assert_eq!(msg.answers().len(), 1);
}

#[test]
fn build_error_response_reparses_client_query_after_upstream_style_roundtrip() {
    let query = valid_a_query_wire();
    let (wire, _, _) = build_error_response(0x00_42, 2, &query, None);
    let msg = Message::from_vec(&wire).unwrap();
    assert_eq!(
        msg.queries().first().unwrap().name().to_utf8(),
        "www.example.com."
    );
    assert_eq!(msg.response_code(), ResponseCode::ServFail);
}
