//! Parse DNS query wire into transaction fields.

use crate::parse_reject::ParseRejectReason;
use crate::phase::Phase;
use crate::pipeline::{PipelineStage, StageOutcome};
use crate::snapshot::RuntimeSnapshot;
use crate::transaction::Transaction;
use hickory_proto::op::{Message, MessageType};
use hickory_proto::rr::RecordType;
use std::sync::Arc;

pub struct ParseStage;

impl ParseStage {
    fn drop_with(txn: &mut Transaction, reason: ParseRejectReason) -> StageOutcome {
        txn.parse_reject_reason = Some(reason);
        StageOutcome::Drop
    }
}

impl PipelineStage for ParseStage {
    fn name(&self) -> &'static str {
        "parse"
    }

    fn handle(&self, txn: &mut Transaction, _snapshot: &Arc<RuntimeSnapshot>) -> StageOutcome {
        if txn.query_wire.is_empty() {
            return Self::drop_with(txn, ParseRejectReason::Empty);
        }
        let message = match Message::from_vec(&txn.query_wire) {
            Ok(m) => m,
            Err(_) => return Self::drop_with(txn, ParseRejectReason::WireError),
        };
        if message.header().message_type() != MessageType::Query {
            return Self::drop_with(txn, ParseRejectReason::NotQuery);
        }
        let queries = message.queries();
        if queries.is_empty() {
            return Self::drop_with(txn, ParseRejectReason::NoQuestion);
        }
        if queries.len() > 1 {
            return Self::drop_with(txn, ParseRejectReason::MultiQuestion);
        }
        txn.dns_id = message.id();
        let query = &queries[0];
        txn.qname = Some(query.name().to_utf8());
        txn.qtype = Some(record_type_to_u16(query.query_type()));
        txn.qclass = Some(u16::from(query.query_class()));
        if message.extensions().is_some() {
            txn.client_udp_payload_size = Some(message.max_payload());
        }
        StageOutcome::Continue(Phase::RequestRules)
    }
}

fn record_type_to_u16(rt: RecordType) -> u16 {
    match rt {
        RecordType::A => 1,
        RecordType::AAAA => 28,
        RecordType::TXT => 16,
        RecordType::MX => 15,
        RecordType::NS => 2,
        RecordType::CNAME => 5,
        RecordType::SOA => 6,
        RecordType::PTR => 12,
        RecordType::SRV => 33,
        _ => 255,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse_reject::ParseRejectReason;
    use crate::snapshot::RuntimeSnapshot;
    use crate::transaction::ClientProtocol;
    use conduit_config::load_yaml;
    use hickory_proto::op::{Message, Query};
    use hickory_proto::rr::{Name, RecordType};
    use hickory_proto::serialize::binary::{BinEncodable, BinEncoder};
    use std::net::SocketAddr;

    fn example_query() -> Vec<u8> {
        let name = Name::from_utf8("www.example.com.").unwrap();
        let query = Query::query(name, RecordType::A);
        let mut msg = Message::new();
        msg.add_query(query);
        let mut buf = Vec::new();
        let mut encoder = BinEncoder::new(&mut buf);
        msg.emit(&mut encoder).unwrap();
        buf
    }

    fn encode_message(msg: Message) -> Vec<u8> {
        let mut buf = Vec::new();
        let mut encoder = BinEncoder::new(&mut buf);
        msg.emit(&mut encoder).unwrap();
        buf
    }

    #[test]
    fn parse_valid_query() {
        let yaml = include_str!("../../../../tests/fixtures/config/minimal.yaml");
        let cfg = load_yaml(yaml).unwrap();
        let snap = Arc::new(RuntimeSnapshot::from_config(cfg));
        let mut txn = Transaction::new(
            1,
            "127.0.0.1:15353".parse::<SocketAddr>().unwrap(),
            ClientProtocol::Udp,
        )
        .with_query_wire(example_query());
        let stage = ParseStage;
        let outcome = stage.handle(&mut txn, &snap);
        assert_eq!(outcome, StageOutcome::Continue(Phase::RequestRules));
        assert!(txn
            .qname
            .as_deref()
            .is_some_and(|q| q.trim_end_matches('.') == "www.example.com"));
        assert_eq!(txn.qclass, Some(1));
    }

    #[test]
    fn parse_rejects_empty() {
        let yaml = include_str!("../../../../tests/fixtures/config/minimal.yaml");
        let cfg = load_yaml(yaml).unwrap();
        let snap = Arc::new(RuntimeSnapshot::from_config(cfg));
        let mut txn = Transaction::new(1, "127.0.0.1:15353".parse().unwrap(), ClientProtocol::Udp);
        assert_eq!(ParseStage.handle(&mut txn, &snap), StageOutcome::Drop);
        assert_eq!(txn.parse_reject_reason, Some(ParseRejectReason::Empty));
    }

    #[test]
    fn parse_rejects_wire_error() {
        let yaml = include_str!("../../../../tests/fixtures/config/minimal.yaml");
        let cfg = load_yaml(yaml).unwrap();
        let snap = Arc::new(RuntimeSnapshot::from_config(cfg));
        let mut txn = Transaction::new(1, "127.0.0.1:15353".parse().unwrap(), ClientProtocol::Udp)
            .with_query_wire(vec![0xff, 0x00, 0x01]);
        assert_eq!(ParseStage.handle(&mut txn, &snap), StageOutcome::Drop);
        assert_eq!(txn.parse_reject_reason, Some(ParseRejectReason::WireError));
    }

    #[test]
    fn parse_rejects_not_query() {
        let yaml = include_str!("../../../../tests/fixtures/config/minimal.yaml");
        let cfg = load_yaml(yaml).unwrap();
        let snap = Arc::new(RuntimeSnapshot::from_config(cfg));
        let name = Name::from_utf8("www.example.com.").unwrap();
        let query = Query::query(name, RecordType::A);
        let mut msg = Message::new();
        msg.add_query(query);
        msg.set_message_type(MessageType::Response);
        let mut txn = Transaction::new(1, "127.0.0.1:15353".parse().unwrap(), ClientProtocol::Udp)
            .with_query_wire(encode_message(msg));
        assert_eq!(ParseStage.handle(&mut txn, &snap), StageOutcome::Drop);
        assert_eq!(txn.parse_reject_reason, Some(ParseRejectReason::NotQuery));
    }

    #[test]
    fn parse_rejects_no_question() {
        let yaml = include_str!("../../../../tests/fixtures/config/minimal.yaml");
        let cfg = load_yaml(yaml).unwrap();
        let snap = Arc::new(RuntimeSnapshot::from_config(cfg));
        let msg = Message::new();
        let mut txn = Transaction::new(1, "127.0.0.1:15353".parse().unwrap(), ClientProtocol::Udp)
            .with_query_wire(encode_message(msg));
        assert_eq!(ParseStage.handle(&mut txn, &snap), StageOutcome::Drop);
        assert_eq!(txn.parse_reject_reason, Some(ParseRejectReason::NoQuestion));
    }

    #[test]
    fn parse_rejects_recursive_name_compression_pointer() {
        let yaml = include_str!("../../../../tests/fixtures/config/minimal.yaml");
        let cfg = load_yaml(yaml).unwrap();
        let snap = Arc::new(RuntimeSnapshot::from_config(cfg));
        // QNAME is a pointer to its own offset (invalid recursive compression).
        let wire = vec![
            0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xC0, 0x0C,
            0x00, 0x01, 0x00, 0x01,
        ];
        let mut txn = Transaction::new(1, "127.0.0.1:15353".parse().unwrap(), ClientProtocol::Udp)
            .with_query_wire(wire);
        assert_eq!(ParseStage.handle(&mut txn, &snap), StageOutcome::Drop);
        assert_eq!(txn.parse_reject_reason, Some(ParseRejectReason::WireError));
    }

    #[test]
    fn parse_rejects_multi_question() {
        let yaml = include_str!("../../../../tests/fixtures/config/minimal.yaml");
        let cfg = load_yaml(yaml).unwrap();
        let snap = Arc::new(RuntimeSnapshot::from_config(cfg));
        let name = Name::from_utf8("www.example.com.").unwrap();
        let mut msg = Message::new();
        msg.add_query(Query::query(name.clone(), RecordType::A));
        msg.add_query(Query::query(name, RecordType::AAAA));
        let mut txn = Transaction::new(1, "127.0.0.1:15353".parse().unwrap(), ClientProtocol::Udp)
            .with_query_wire(encode_message(msg));
        assert_eq!(ParseStage.handle(&mut txn, &snap), StageOutcome::Drop);
        assert_eq!(
            txn.parse_reject_reason,
            Some(ParseRejectReason::MultiQuestion)
        );
    }
}
