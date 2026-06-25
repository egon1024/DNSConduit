//! Parse DNS query wire into transaction fields.

use crate::parse_reject::ParseRejectReason;
use crate::phase::Phase;
use crate::pipeline::{PipelineStage, StageOutcome};
use crate::snapshot::RuntimeSnapshot;
use crate::structural_parse::{apply_parsed_query, structural_parse};
use crate::transaction::Transaction;
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
        if txn.pre_parsed {
            if txn.qname.is_none() {
                return Self::drop_with(txn, ParseRejectReason::WireError);
            }
            return StageOutcome::Continue(Phase::RequestRules);
        }
        match structural_parse(&txn.query_wire) {
            Ok(parsed) => {
                apply_parsed_query(txn, parsed);
                StageOutcome::Continue(Phase::RequestRules)
            }
            Err(reason) => Self::drop_with(txn, reason),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse_reject::ParseRejectReason;
    use crate::snapshot::RuntimeSnapshot;
    use crate::transaction::ClientProtocol;
    use conduit_config::load_yaml;
    use hickory_proto::op::{Message, MessageType, Query};
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
    fn parse_skips_wire_when_pre_parsed() {
        let yaml = include_str!("../../../../tests/fixtures/config/minimal.yaml");
        let cfg = load_yaml(yaml).unwrap();
        let snap = Arc::new(RuntimeSnapshot::from_config(cfg));
        let mut txn = Transaction::new(
            1,
            "127.0.0.1:15353".parse::<SocketAddr>().unwrap(),
            ClientProtocol::Udp,
        );
        txn.pre_parsed = true;
        txn.qname = Some("www.example.com.".into());
        txn.qtype = Some(1);
        txn.qclass = Some(1);
        txn.dns_id = 42;
        assert_eq!(
            ParseStage.handle(&mut txn, &snap),
            StageOutcome::Continue(Phase::RequestRules)
        );
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
        assert!(txn.pre_parsed);
        assert!(txn
            .qname
            .as_deref()
            .is_some_and(|q| q.trim_end_matches('.') == "www.example.com"));
        assert_eq!(txn.qclass, Some(1));
        assert_eq!(txn.opcode, Some(0));
        assert!(txn.edns_option_codes.is_empty());
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
