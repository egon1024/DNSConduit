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
