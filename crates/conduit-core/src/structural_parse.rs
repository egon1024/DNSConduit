//! Structural DNS query parse (header + question + EDNS) without policy semantics.

use crate::parse_reject::ParseRejectReason;
use crate::transaction::Transaction;
use hickory_proto::op::{Message, MessageType};

/// Fields extracted from a structurally valid DNS query.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedQuery {
    pub dns_id: u16,
    pub opcode: u8,
    pub qname: String,
    pub qtype: u16,
    pub qclass: u16,
    pub client_udp_payload_size: Option<u16>,
    pub edns_option_codes: Vec<u16>,
}

/// Parse wire into query metadata. Does not mutate a transaction.
pub fn structural_parse(wire: &[u8]) -> Result<ParsedQuery, ParseRejectReason> {
    if wire.is_empty() {
        return Err(ParseRejectReason::Empty);
    }
    let message = match Message::from_vec(wire) {
        Ok(m) => m,
        Err(_) => return Err(ParseRejectReason::WireError),
    };
    if message.header().message_type() != MessageType::Query {
        return Err(ParseRejectReason::NotQuery);
    }
    let queries = message.queries();
    if queries.is_empty() {
        return Err(ParseRejectReason::NoQuestion);
    }
    if queries.len() > 1 {
        return Err(ParseRejectReason::MultiQuestion);
    }
    let query = &queries[0];
    let mut edns_option_codes = Vec::new();
    let client_udp_payload_size = message.extensions().as_ref().map(|edns| {
        edns_option_codes = edns
            .options()
            .as_ref()
            .keys()
            .map(|code| u16::from(*code))
            .collect();
        edns_option_codes.sort_unstable();
        edns_option_codes.dedup();
        edns.max_payload()
    });
    Ok(ParsedQuery {
        dns_id: message.id(),
        opcode: u8::from(message.header().op_code()),
        qname: query.name().to_utf8(),
        qtype: u16::from(query.query_type()),
        qclass: u16::from(query.query_class()),
        client_udp_payload_size,
        edns_option_codes,
    })
}

/// Apply structural parse output to a transaction and mark `pre_parsed`.
pub fn apply_parsed_query(txn: &mut Transaction, parsed: ParsedQuery) {
    txn.dns_id = parsed.dns_id;
    txn.opcode = Some(parsed.opcode);
    txn.qname = Some(parsed.qname);
    txn.qtype = Some(parsed.qtype);
    txn.qclass = Some(parsed.qclass);
    txn.client_udp_payload_size = parsed.client_udp_payload_size;
    txn.edns_option_codes = parsed.edns_option_codes;
    txn.pre_parsed = true;
}

#[cfg(test)]
mod tests {
    use super::*;
    use hickory_proto::op::{Message, Query};
    use hickory_proto::rr::{Name, RecordType};
    use hickory_proto::serialize::binary::{BinEncodable, BinEncoder};

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
    fn structural_parse_valid_query() {
        let parsed = structural_parse(&example_query()).unwrap();
        assert_eq!(parsed.dns_id, 0);
        assert_eq!(parsed.opcode, 0);
        assert!(parsed.qname.trim_end_matches('.').eq("www.example.com"));
        assert_eq!(parsed.qtype, 1);
        assert_eq!(parsed.qclass, 1);
        assert!(parsed.edns_option_codes.is_empty());
    }

    #[test]
    fn structural_parse_rejects_empty() {
        assert_eq!(structural_parse(&[]), Err(ParseRejectReason::Empty));
    }

    #[test]
    fn structural_parse_rejects_garbage() {
        assert_eq!(
            structural_parse(&[0xff, 0x00, 0x01]),
            Err(ParseRejectReason::WireError)
        );
    }

    #[test]
    fn structural_parse_rejects_not_query() {
        let name = Name::from_utf8("www.example.com.").unwrap();
        let query = Query::query(name, RecordType::A);
        let mut msg = Message::new();
        msg.add_query(query);
        msg.set_message_type(MessageType::Response);
        assert_eq!(
            structural_parse(&encode_message(msg)),
            Err(ParseRejectReason::NotQuery)
        );
    }

    #[test]
    fn apply_parsed_query_sets_selector_fields() {
        use crate::transaction::{ClientProtocol, Transaction};
        use std::net::SocketAddr;

        let mut txn = Transaction::new(
            1,
            "127.0.0.1:53".parse::<SocketAddr>().unwrap(),
            ClientProtocol::Udp,
        );
        apply_parsed_query(
            &mut txn,
            ParsedQuery {
                dns_id: 9,
                opcode: 0,
                qname: "test.example.".into(),
                qtype: 1,
                qclass: 1,
                client_udp_payload_size: Some(1232),
                edns_option_codes: vec![8],
            },
        );
        assert!(txn.pre_parsed);
        assert_eq!(txn.dns_id, 9);
        assert_eq!(txn.opcode, Some(0));
        assert_eq!(txn.qname.as_deref(), Some("test.example."));
        assert_eq!(txn.qtype, Some(1));
        assert_eq!(txn.qclass, Some(1));
        assert_eq!(txn.client_udp_payload_size, Some(1232));
        assert_eq!(txn.edns_option_codes, vec![8]);
    }
}
