//! DNS wire parsing for tap output.

use hickory_proto::op::{Message, OpCode};
use hickory_proto::rr::Record;
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct DnsDetail {
    pub header: HeaderDetail,
    pub question: Option<QuestionDetail>,
    pub answers: Vec<String>,
    pub authority: Vec<String>,
    pub additional: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct HeaderDetail {
    pub id: u16,
    pub opcode: String,
    pub rcode: Option<String>,
    pub qr: bool,
    pub aa: bool,
    pub tc: bool,
    pub rd: bool,
    pub ra: bool,
    pub ad: bool,
    pub cd: bool,
    pub query_count: u16,
    pub answer_count: u16,
    pub authority_count: u16,
    pub additional_count: u16,
}

#[derive(Debug, Clone, Serialize)]
pub struct QuestionDetail {
    pub name: String,
    pub qtype: String,
    pub qclass: String,
}

pub fn parse_dns_wire(wire: &[u8]) -> Option<DnsDetail> {
    let msg = Message::from_vec(wire).ok()?;
    let header = msg.header();
    let qr = matches!(
        header.message_type(),
        hickory_proto::op::MessageType::Response
    );

    let header_detail = HeaderDetail {
        id: header.id(),
        opcode: format_opcode(header.op_code()),
        rcode: qr.then(|| header.response_code().to_string()),
        qr,
        aa: header.authoritative(),
        tc: header.truncated(),
        rd: header.recursion_desired(),
        ra: header.recursion_available(),
        ad: header.authentic_data(),
        cd: header.checking_disabled(),
        query_count: header.query_count(),
        answer_count: header.answer_count(),
        authority_count: header.name_server_count(),
        additional_count: header.additional_count(),
    };

    let question = msg.queries().first().map(|q| QuestionDetail {
        name: q.name().to_utf8(),
        qtype: q.query_type().to_string(),
        qclass: q.query_class().to_string(),
    });

    Some(DnsDetail {
        header: header_detail,
        question,
        answers: format_records(msg.answers()),
        authority: format_records(msg.name_servers()),
        additional: format_records(msg.additionals()),
    })
}

fn format_opcode(op: OpCode) -> String {
    op.to_string()
}

fn format_records(records: &[Record]) -> Vec<String> {
    records.iter().map(|r| r.to_string()).collect()
}

#[cfg(test)]
mod tests {
    use super::parse_dns_wire;
    use hickory_proto::op::{Message, MessageType, Query, ResponseCode};
    use hickory_proto::rr::rdata::A;
    use hickory_proto::rr::{Name, RData, Record, RecordType};
    use hickory_proto::serialize::binary::{BinEncodable, BinEncoder};

    fn response_with_compressed_answer() -> Vec<u8> {
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
        let mut buf = Vec::new();
        let mut enc = BinEncoder::new(&mut buf);
        msg.emit(&mut enc).unwrap();
        assert!(buf.windows(2).any(|w| w[0] & 0xC0 == 0xC0));
        buf
    }

    #[test]
    fn parse_dns_wire_decompresses_question_from_compressed_response() {
        let detail = parse_dns_wire(&response_with_compressed_answer()).expect("must parse");
        let q = detail.question.expect("question present");
        assert_eq!(q.name, "www.example.com.");
        assert_eq!(q.qtype, "A");
        assert_eq!(detail.answers.len(), 1);
        assert!(detail.answers[0].contains("93.184.215.14"));
    }

    #[test]
    fn parse_dns_wire_handles_hickory_regression_fixture() {
        #[rustfmt::skip]
        let wire: Vec<u8> = vec![
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
        let detail = parse_dns_wire(&wire).expect("fixed bytes must parse");
        assert_eq!(detail.header.id, 4096);
        assert_eq!(
            detail.question.as_ref().map(|q| q.name.as_str()),
            Some("www.example.com.")
        );
        assert_eq!(detail.answers.len(), 1);
    }
}
