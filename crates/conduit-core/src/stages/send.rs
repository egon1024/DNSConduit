//! Send phase — ensure response wire is available (built in parse/forward path).

use crate::phase::Phase;
use crate::pipeline::{PipelineStage, StageOutcome};
use crate::snapshot::RuntimeSnapshot;
use crate::transaction::Transaction;
use hickory_proto::op::{Message, MessageType, ResponseCode};
use hickory_proto::serialize::binary::{BinEncodable, BinEncoder};
use std::sync::Arc;

pub struct SendStage;

impl PipelineStage for SendStage {
    fn name(&self) -> &'static str {
        "send"
    }

    fn handle(&self, txn: &mut Transaction, _snapshot: &Arc<RuntimeSnapshot>) -> StageOutcome {
        if txn.response_wire.is_some() {
            return StageOutcome::Continue(Phase::Send);
        }
        let rcode = txn.rcode().unwrap_or(2);
        let wire = build_error_response(
            txn.dns_id,
            rcode,
            &txn.query_wire,
            txn.client_udp_payload_size,
        );
        txn.response_wire = Some(wire);
        StageOutcome::Continue(Phase::Send)
    }
}

pub fn build_error_response(
    id: u16,
    rcode: u16,
    query_wire: &[u8],
    udp_payload_size: Option<u16>,
) -> Vec<u8> {
    let mut msg = Message::new();
    msg.set_id(id);
    msg.set_message_type(MessageType::Response);
    if !query_wire.is_empty() {
        if let Ok(query) = Message::from_vec(query_wire) {
            for q in query.queries() {
                msg.add_query(q.clone());
            }
            if let Some(edns) = query.extensions().clone() {
                msg.set_edns(edns);
            }
        }
    }
    msg.set_response_code(ResponseCode::from_low((rcode & 0xf) as u8));
    msg.set_authoritative(true);
    let mut buf = Vec::new();
    let mut encoder = BinEncoder::new(&mut buf);
    let _ = msg.emit(&mut encoder);
    truncate_udp(&mut buf, udp_payload_size);
    buf
}

fn truncate_udp(buf: &mut Vec<u8>, udp_payload_size: Option<u16>) {
    let limit = udp_payload_size.unwrap_or(512) as usize;
    if buf.len() > limit {
        buf.truncate(limit);
        if buf.len() >= 3 {
            buf[2] |= 0x02; // TC bit
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hickory_proto::op::{Message, Query};
    use hickory_proto::rr::{Name, RecordType};
    use hickory_proto::serialize::binary::{BinEncodable, BinEncoder};

    fn example_query() -> Vec<u8> {
        let name = Name::from_utf8("foo.example.com.").unwrap();
        let query = Query::query(name, RecordType::A);
        let mut msg = Message::new();
        msg.add_query(query);
        let mut buf = Vec::new();
        let mut encoder = BinEncoder::new(&mut buf);
        msg.emit(&mut encoder).unwrap();
        buf
    }

    #[test]
    fn error_response_parses_query_after_compressed_upstream_response_roundtrip() {
        use hickory_proto::op::{MessageType, ResponseCode};
        use hickory_proto::rr::rdata::A;
        use hickory_proto::rr::{RData, Record};

        let name = Name::from_utf8("www.example.com.").unwrap();
        let mut upstream = Message::new();
        upstream.set_id(0x00_42);
        upstream.set_message_type(MessageType::Response);
        upstream.set_response_code(ResponseCode::NoError);
        upstream.add_query(Query::query(name.clone(), RecordType::A));
        upstream.add_answer(Record::from_rdata(
            name.clone(),
            60,
            RData::A(A::new(1, 2, 3, 4)),
        ));
        let mut upstream_wire = Vec::new();
        let mut enc = BinEncoder::new(&mut upstream_wire);
        upstream.emit(&mut enc).unwrap();
        assert!(upstream_wire.windows(2).any(|w| w[0] & 0xC0 == 0xC0));

        let mut client_query = Message::new();
        client_query.set_id(0x00_42);
        client_query.add_query(Query::query(name, RecordType::A));
        let mut query_wire = Vec::new();
        let mut qenc = BinEncoder::new(&mut query_wire);
        client_query.emit(&mut qenc).unwrap();

        let _ = Message::from_vec(&upstream_wire).expect("compressed response parses");
        let wire = build_error_response(0x00_42, 5, &query_wire, None);
        let parsed = Message::from_vec(&wire).unwrap();
        assert_eq!(
            parsed.queries().first().unwrap().name().to_utf8(),
            "www.example.com."
        );
        assert_eq!(parsed.response_code(), ResponseCode::Refused);
    }

    #[test]
    fn error_response_respects_512_without_edns() {
        let wire = build_error_response(42, 2, &example_query(), None);
        assert!(wire.len() <= 512);
        assert_eq!(wire[0], 0);
        assert_eq!(wire[1], 42);
        let parsed = Message::from_vec(&wire).unwrap();
        assert_eq!(parsed.queries().len(), 1);
        assert!(parsed.header().message_type() == MessageType::Response);
    }
}
