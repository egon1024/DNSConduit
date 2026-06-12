//! DNS wire builders for label-compression regression tests.

use hickory_proto::op::{Message, MessageType, Query, ResponseCode};
use hickory_proto::rr::rdata::A;
use hickory_proto::rr::{Name, RData, Record, RecordType};
use hickory_proto::serialize::binary::{BinEncodable, BinEncoder};

/// True when the wire contains a DNS name compression pointer (`11` high bits set).
pub fn wire_contains_compression_pointer(wire: &[u8]) -> bool {
    wire.windows(2).any(|w| w[0] & 0xC0 == 0xC0 && w[0] != 0x00)
}

fn emit(msg: &Message) -> Vec<u8> {
    let mut buf = Vec::new();
    let mut enc = BinEncoder::new(&mut buf);
    msg.emit(&mut enc).expect("fixture encode");
    buf
}

/// Standard uncompressed A query for `www.example.com`.
pub fn valid_a_query_wire() -> Vec<u8> {
    let name = Name::from_utf8("www.example.com.").unwrap();
    let mut msg = Message::new();
    msg.set_id(0x00_42);
    msg.add_query(Query::query(name, RecordType::A));
    emit(&msg)
}

/// NOERROR response whose answer RR name compresses to the question (`0xC0 0x0C` pattern).
pub fn response_with_compressed_answer_name() -> Vec<u8> {
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
    assert!(
        wire_contains_compression_pointer(&wire),
        "fixture must exercise name compression; wire={wire:02x?}"
    );
    wire
}

/// Fixed regression bytes (hickory `test_legit_message`): answer name is a pointer to qname.
pub fn hickory_compressed_response_bytes() -> Vec<u8> {
    #[rustfmt::skip]
    let buf: Vec<u8> = vec![
        0x10, 0x00, 0x81, 0x80,
        0x00, 0x01, 0x00, 0x01,
        0x00, 0x00, 0x00, 0x00,
        0x03, b'w', b'w', b'w',
        0x07, b'e', b'x', b'a',
        b'm', b'p', b'l', b'e',
        0x03, b'c', b'o', b'm',
        0x00,
        0x00, 0x01, 0x00, 0x01,
        0xC0, 0x0C,
        0x00, 0x01, 0x00, 0x01,
        0x00, 0x00, 0x00, 0x02,
        0x00, 0x04,
        0x5D, 0xB8, 0xD7, 0x0E,
    ];
    assert!(wire_contains_compression_pointer(&buf));
    buf
}

/// Query whose qname is a forward compression pointer (invalid on the wire).
pub fn query_with_forward_name_pointer() -> Vec<u8> {
    vec![
        0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xC0, 0x30, 0x00,
        0x01, 0x00, 0x01,
    ]
}

/// Query whose qname pointer targets itself (recursive compression; invalid).
pub fn query_with_recursive_name_pointer() -> Vec<u8> {
    vec![
        0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xC0, 0x0C, 0x00,
        0x01, 0x00, 0x01,
    ]
}
