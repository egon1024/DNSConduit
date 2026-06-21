//! Upstream DNS response metadata extracted after forward (when enabled at compile time).

use crate::transaction::Transaction;
use conduit_script::ResponseWireMeta;
use hickory_proto::op::Message;

/// Record upstream response on the transaction: always sets RCODE when wire parses; optional section meta.
pub fn record_upstream_response(txn: &mut Transaction, wire: &[u8], parse_wire_meta: bool) {
    if let Ok(msg) = Message::from_vec(wire) {
        txn.set_rcode(msg.response_code().low() as u16);
        if parse_wire_meta {
            txn.response_meta = Some(ResponseWireMeta {
                answer_count: u16::try_from(msg.answers().len()).unwrap_or(u16::MAX),
                authority_count: u16::try_from(msg.name_servers().len()).unwrap_or(u16::MAX),
                additional_count: u16::try_from(msg.additionals().len()).unwrap_or(u16::MAX),
                truncated: msg.header().truncated(),
                authoritative: msg.header().authoritative(),
            });
        } else {
            txn.response_meta = None;
        }
    } else if parse_wire_meta {
        txn.response_meta = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transaction::{ClientProtocol, Transaction};
    use hickory_proto::op::{Message, Query};
    use hickory_proto::rr::{Name, RData, Record, RecordType};
    use hickory_proto::serialize::binary::{BinEncodable, BinEncoder};
    use std::net::SocketAddr;

    fn encode(msg: Message) -> Vec<u8> {
        let mut buf = Vec::new();
        let mut enc = BinEncoder::new(&mut buf);
        msg.emit(&mut enc).unwrap();
        buf
    }

    #[test]
    fn record_rcode_without_wire_meta() {
        let mut msg = Message::new();
        msg.set_response_code(hickory_proto::op::ResponseCode::NXDomain);
        let wire = encode(msg);
        let mut txn = Transaction::new(1, "127.0.0.1:53".parse::<SocketAddr>().unwrap(), ClientProtocol::Udp);
        record_upstream_response(&mut txn, &wire, false);
        assert_eq!(txn.rcode(), Some(3));
        assert!(txn.response_meta.is_none());
    }

    #[test]
    fn record_wire_meta_when_enabled() {
        let name = Name::from_utf8("example.com.").unwrap();
        let mut msg = Message::new();
        msg.add_query(Query::query(name.clone(), RecordType::A));
        msg.add_answer(Record::from_rdata(
            name,
            300,
            RData::A(hickory_proto::rr::rdata::A(std::net::Ipv4Addr::new(93, 184, 216, 34))),
        ));
        msg.set_authoritative(true);
        let wire = encode(msg);
        let mut txn = Transaction::new(1, "127.0.0.1:53".parse::<SocketAddr>().unwrap(), ClientProtocol::Udp);
        record_upstream_response(&mut txn, &wire, true);
        let meta = txn.response_meta.expect("meta");
        assert_eq!(meta.answer_count, 1);
        assert!(meta.authoritative);
        assert!(!meta.truncated);
    }
}
