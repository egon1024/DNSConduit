//! Forward-path regression tests for DNS label compression.

use conduit_config::forward::RecursionDesired;
use conduit_dataplane::forward::rd::build_upstream_wire;
use hickory_proto::op::{Message, MessageType, Query, ResponseCode};
use hickory_proto::rr::rdata::A;
use hickory_proto::rr::{Name, RData, Record, RecordType};
use hickory_proto::serialize::binary::{BinEncodable, BinEncoder};

fn emit(msg: &Message) -> Vec<u8> {
    let mut buf = Vec::new();
    let mut enc = BinEncoder::new(&mut buf);
    msg.emit(&mut enc).unwrap();
    buf
}

fn valid_a_query_wire() -> Vec<u8> {
    let name = Name::from_utf8("www.example.com.").unwrap();
    let mut msg = Message::new();
    msg.set_id(0x00_42);
    msg.add_query(Query::query(name, RecordType::A));
    emit(&msg)
}

fn query_with_recursive_name_pointer() -> Vec<u8> {
    vec![
        0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xC0, 0x0C, 0x00,
        0x01, 0x00, 0x01,
    ]
}

fn response_with_compressed_answer_name() -> Vec<u8> {
    let name = Name::from_utf8("www.example.com.").unwrap();
    let mut msg = Message::new();
    msg.set_id(0x10_00);
    msg.set_message_type(MessageType::Response);
    msg.set_response_code(ResponseCode::NoError);
    msg.add_query(Query::query(name.clone(), RecordType::A));
    msg.add_answer(Record::from_rdata(
        name,
        300,
        RData::A(A::new(93, 184, 215, 14)),
    ));
    let wire = emit(&msg);
    assert!(wire.windows(2).any(|w| w[0] & 0xC0 == 0xC0));
    wire
}

#[test]
fn rd_preserve_keeps_unparseable_compression_wire_unchanged() {
    let wire = query_with_recursive_name_pointer();
    let out = build_upstream_wire(&wire, RecursionDesired::Preserve);
    assert_eq!(out, wire);
}

#[test]
fn rd_clear_reparses_valid_query_without_corrupting_qname() {
    let wire = valid_a_query_wire();
    let out = build_upstream_wire(&wire, RecursionDesired::Clear);
    let msg = Message::from_vec(&out).unwrap();
    assert!(!msg.header().recursion_desired());
    assert_eq!(
        msg.queries().first().unwrap().name().to_utf8(),
        "www.example.com."
    );
}

/// Mirrors `ForwardTransport::finish_response` rcode extraction on compressed upstream wire.
#[test]
fn upstream_response_with_compressed_rr_name_yields_rcode() {
    let wire = response_with_compressed_answer_name();
    let msg = Message::from_vec(&wire).expect("forward path must parse compressed responses");
    assert_eq!(msg.response_code().low() as u16, 0);
}
