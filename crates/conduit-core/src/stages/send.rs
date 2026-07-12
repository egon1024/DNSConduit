//! Send phase — ensure response wire is available (built in parse/forward path).

use crate::phase::Phase;
use crate::pipeline::{PipelineStage, StageOutcome};
use crate::snapshot::RuntimeSnapshot;
use crate::transaction::{ClientProtocol, Transaction};
use hickory_proto::op::{Message, MessageType, ResponseCode};
use hickory_proto::serialize::binary::{BinEncodable, BinEncoder};
use std::sync::Arc;

pub struct SendStage;

impl PipelineStage for SendStage {
    fn name(&self) -> &'static str {
        "send"
    }

    fn handle(&self, txn: &mut Transaction, _snapshot: &Arc<RuntimeSnapshot>) -> StageOutcome {
        if let Some(mut wire) = txn.response_wire.take() {
            if txn.protocol == ClientProtocol::Udp {
                let before_len = wire.len();
                let payload_limit = txn.client_udp_payload_size.unwrap_or(512);
                if truncate_udp(&mut wire, txn.client_udp_payload_size) {
                    txn.udp_response_truncated_on_send = true;
                    log_udp_truncation(txn, before_len, wire.len(), payload_limit);
                }
            }
            txn.response_wire = Some(wire);
            return StageOutcome::Continue(Phase::Send);
        }
        let rcode = txn.rcode().unwrap_or(2);
        let payload_limit = txn.client_udp_payload_size.unwrap_or(512);
        let (wire, truncated, before_len) = build_error_response(
            txn.dns_id,
            rcode,
            &txn.query_wire,
            txn.client_udp_payload_size,
        );
        if truncated {
            txn.udp_response_truncated_on_send = true;
            log_udp_truncation(txn, before_len, wire.len(), payload_limit);
        }
        txn.response_wire = Some(wire);
        StageOutcome::Continue(Phase::Send)
    }
}

fn log_udp_truncation(
    txn: &Transaction,
    wire_len_before: usize,
    wire_len_after: usize,
    payload_limit: u16,
) {
    tracing::debug!(
        txn_id = txn.id,
        listener = txn.listener_label.as_deref().unwrap_or("unknown"),
        answer_source = ?txn.answer_source,
        client_udp_payload_size = payload_limit,
        wire_len_before,
        wire_len_after,
        "udp response fitted to client payload size on RR boundaries; TC set"
    );
}

pub fn build_error_response(
    id: u16,
    rcode: u16,
    query_wire: &[u8],
    udp_payload_size: Option<u16>,
) -> (Vec<u8>, bool, usize) {
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
    let before_len = buf.len();
    let truncated = truncate_udp(&mut buf, udp_payload_size);
    (buf, truncated, before_len)
}

/// Fit a DNS response into the UDP payload limit without cutting mid-RR.
///
/// Follows RFC 2181 §9 / §5.1 intent:
/// - Prefer dropping optional additional (then authority) RRs with **TC clear**
///   when the required answer still fits.
/// - If a required answer RRset cannot fit in full, set **TC** and may leave a
///   prefix of complete RRs (never a partial RR on the wire).
/// - EDNS OPT is preserved when present.
///
/// Returns `true` when the served wire has TC set.
fn truncate_udp(buf: &mut Vec<u8>, udp_payload_size: Option<u16>) -> bool {
    let limit = udp_payload_size.unwrap_or(512) as usize;
    if buf.len() <= limit {
        return false;
    }

    let Ok(mut msg) = Message::from_vec(buf) else {
        // Unparseable wire: last-resort clip so UDP still respects the limit.
        buf.truncate(limit);
        if buf.len() >= 3 {
            buf[2] |= 0x02;
        }
        return true;
    };

    // Preserve EDNS separately; hickory keeps OPT out of `additionals()` when
    // extensions are set, but clear both to be safe then restore EDNS.
    let edns = msg.extensions().clone();
    let had_answers = !msg.answers().is_empty();
    let had_authority = !msg.name_servers().is_empty();

    // 1) Drop optional additional RRs (RFC 2181: do not set TC for these alone).
    msg.additionals_mut().clear();
    if let Some(edns) = edns.clone() {
        msg.set_edns(edns);
    }
    if let Some(wire) = emit_if_fits(&msg, limit) {
        *buf = wire;
        return false;
    }

    // 2) Drop authority next. For positive answers this is usually optional
    //    (TC clear). For empty-answer replies (NXDOMAIN / referral) authority
    //    is required content — dropping it means TC must be set.
    msg.name_servers_mut().clear();
    if had_authority && !had_answers {
        msg.set_truncated(true);
    }
    if let Some(edns) = edns.clone() {
        msg.set_edns(edns);
    }
    if let Some(wire) = emit_if_fits(&msg, limit) {
        *buf = wire;
        return had_authority && !had_answers;
    }

    // 3) Required answer data still too large: set TC and trim complete answer RRs.
    msg.set_truncated(true);
    while !msg.answers().is_empty() {
        if let Some(edns) = edns.clone() {
            msg.set_edns(edns);
        }
        if let Some(wire) = emit_if_fits(&msg, limit) {
            *buf = wire;
            return true;
        }
        msg.answers_mut().pop();
    }

    // Answers empty and still oversized (e.g. huge question/EDNS): emit best effort.
    if let Some(edns) = edns {
        msg.set_edns(edns);
    }
    match emit_message(&msg) {
        Ok(mut wire) => {
            if wire.len() > limit {
                // Extremely pathological: cannot fit header+question+OPT as structured
                // RRs. Fall back to a clipped buffer with TC (should be rare).
                wire.truncate(limit);
                if wire.len() >= 3 {
                    wire[2] |= 0x02;
                }
            }
            *buf = wire;
        }
        Err(_) => {
            buf.truncate(limit);
            if buf.len() >= 3 {
                buf[2] |= 0x02;
            }
        }
    }
    true
}

fn emit_message(msg: &Message) -> Result<Vec<u8>, hickory_proto::error::ProtoError> {
    let mut wire = Vec::new();
    let mut enc = BinEncoder::new(&mut wire);
    msg.emit(&mut enc)?;
    Ok(wire)
}

fn emit_if_fits(msg: &Message, limit: usize) -> Option<Vec<u8>> {
    let wire = emit_message(msg).ok()?;
    if wire.len() <= limit {
        Some(wire)
    } else {
        None
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
        let (wire, _, _) = build_error_response(0x00_42, 5, &query_wire, None);
        let parsed = Message::from_vec(&wire).unwrap();
        assert_eq!(
            parsed.queries().first().unwrap().name().to_utf8(),
            "www.example.com."
        );
        assert_eq!(parsed.response_code(), ResponseCode::Refused);
    }

    #[test]
    fn error_response_respects_512_without_edns() {
        let (wire, _, _) = build_error_response(42, 2, &example_query(), None);
        assert!(wire.len() <= 512);
        assert_eq!(wire[0], 0);
        assert_eq!(wire[1], 42);
        let parsed = Message::from_vec(&wire).unwrap();
        assert_eq!(parsed.queries().len(), 1);
        assert!(parsed.header().message_type() == MessageType::Response);
    }

    #[test]
    fn existing_response_wire_truncated_for_udp_client_payload() {
        use crate::pipeline::PipelineStage;
        use crate::snapshot::RuntimeSnapshot;
        use crate::transaction::{ClientProtocol, Transaction};
        use conduit_config::file::load_yaml;
        use hickory_proto::op::{MessageType, ResponseCode};
        use hickory_proto::rr::rdata::A;
        use hickory_proto::rr::{RData, Record};
        use std::net::SocketAddr;
        use std::sync::Arc;

        let name = Name::from_utf8("tc.policy-lab.test.example.").unwrap();
        let mut msg = Message::new();
        msg.set_id(0x1234);
        msg.set_message_type(MessageType::Response);
        msg.set_response_code(ResponseCode::NoError);
        msg.add_query(Query::query(name.clone(), RecordType::A));
        for i in 1..=40u8 {
            msg.add_answer(Record::from_rdata(
                name.clone(),
                3600,
                RData::A(A::new(192, 0, 2, i)),
            ));
        }
        let mut oversized = Vec::new();
        let mut enc = BinEncoder::new(&mut oversized);
        msg.emit(&mut enc).unwrap();
        assert!(
            oversized.len() > 512,
            "fixture must exceed 512 bytes, got {}",
            oversized.len()
        );

        let addr: SocketAddr = "127.0.0.1:53".parse().unwrap();
        let mut txn = Transaction::new(1, addr, ClientProtocol::Udp);
        txn.client_udp_payload_size = Some(512);
        txn.response_wire = Some(oversized);

        let stage = SendStage;
        let cfg = load_yaml(include_str!(
            "../../../../tests/fixtures/config/minimal.yaml"
        ))
        .unwrap();
        let snap = Arc::new(RuntimeSnapshot::from_config(cfg));
        let _ = stage.handle(&mut txn, &snap);

        let wire = txn.response_wire.expect("send stage must keep wire");
        assert!(wire.len() <= 512, "wire len {}", wire.len());
        assert_ne!(wire[2] & 0x02, 0, "TC bit must be set");
        assert!(txn.udp_response_truncated_on_send);
        let parsed =
            Message::from_vec(&wire).expect("truncated wire must remain a valid DNS message");
        assert!(!parsed.answers().is_empty());
        for rr in parsed.answers() {
            assert_eq!(rr.record_type(), RecordType::A);
        }
    }

    fn encode_msg(msg: &Message) -> Vec<u8> {
        let mut buf = Vec::new();
        let mut enc = BinEncoder::new(&mut buf);
        msg.emit(&mut enc).unwrap();
        buf
    }

    fn multi_a_response(count: u8) -> Message {
        use hickory_proto::op::{MessageType, ResponseCode};
        use hickory_proto::rr::rdata::A;
        use hickory_proto::rr::{RData, Record};

        let name = Name::from_ascii("big.example.").unwrap();
        let mut msg = Message::new();
        msg.set_id(0x2222);
        msg.set_message_type(MessageType::Response);
        msg.set_response_code(ResponseCode::NoError);
        msg.add_query(Query::query(name.clone(), RecordType::A));
        for i in 1..=count {
            msg.add_answer(Record::from_rdata(
                name.clone(),
                300,
                RData::A(A::new(203, 0, 113, i)),
            ));
        }
        msg
    }

    #[test]
    fn truncate_udp_never_cuts_mid_rr_and_sets_tc_for_partial_answer_rrset() {
        let msg = multi_a_response(40);
        let mut wire = encode_msg(&msg);
        let full_len = wire.len();
        assert!(full_len > 512);

        // Choose a limit that would fall inside an RR for naive byte clipping.
        let limit = 200usize;
        assert!(limit < full_len);
        let truncated = truncate_udp(&mut wire, Some(limit as u16));
        assert!(truncated, "oversized answer RRset must set TC");
        assert!(
            wire.len() <= limit,
            "served len {} > limit {}",
            wire.len(),
            limit
        );
        assert_ne!(wire[2] & 0x02, 0);

        let parsed = Message::from_vec(&wire).expect("must not leave a mid-RR fragment");
        assert_eq!(parsed.id(), 0x2222);
        assert!(parsed.truncated());
        assert!(!parsed.answers().is_empty());
        assert!(parsed.answers().len() < 40);
        for rr in parsed.answers() {
            assert_eq!(rr.record_type(), RecordType::A);
            assert_eq!(rr.name().to_utf8(), "big.example.");
        }
    }

    #[test]
    fn truncate_udp_drops_additionals_without_tc_when_answers_fit() {
        use hickory_proto::op::{MessageType, ResponseCode};
        use hickory_proto::rr::rdata::A;
        use hickory_proto::rr::{RData, Record};

        let qname = Name::from_ascii("small.example.").unwrap();
        let extra = Name::from_ascii("glue.example.").unwrap();
        let mut msg = Message::new();
        msg.set_id(0x3333);
        msg.set_message_type(MessageType::Response);
        msg.set_response_code(ResponseCode::NoError);
        msg.add_query(Query::query(qname.clone(), RecordType::A));
        msg.add_answer(Record::from_rdata(
            qname,
            60,
            RData::A(A::new(192, 0, 2, 1)),
        ));
        // Pad additional section so the full message exceeds a tight limit.
        for i in 1..=30u8 {
            msg.add_additional(Record::from_rdata(
                extra.clone(),
                60,
                RData::A(A::new(198, 51, 100, i)),
            ));
        }

        let mut wire = encode_msg(&msg);
        let full_len = wire.len();
        // Limit between "answers only" size and full size.
        let mut answers_only = msg.clone();
        answers_only.additionals_mut().clear();
        let answers_only_len = encode_msg(&answers_only).len();
        assert!(answers_only_len < full_len);
        let limit = answers_only_len + 8;
        assert!(limit < full_len);

        let truncated = truncate_udp(&mut wire, Some(limit as u16));
        assert!(
            !truncated,
            "dropping optional additionals should not set TC when answers fit"
        );
        assert!(wire.len() <= limit);
        let parsed = Message::from_vec(&wire).unwrap();
        assert!(!parsed.truncated());
        assert_eq!(parsed.answers().len(), 1);
        assert!(parsed.additionals().is_empty());
    }

    #[test]
    fn truncate_udp_noop_when_already_within_limit() {
        let msg = multi_a_response(2);
        let mut wire = encode_msg(&msg);
        let original = wire.clone();
        assert!(!truncate_udp(&mut wire, Some(512)));
        assert_eq!(wire, original);
        assert_eq!(wire[2] & 0x02, 0);
    }

    #[test]
    fn truncate_udp_preserves_edns_when_trimming_answers() {
        use hickory_proto::op::{Edns, MessageType, ResponseCode};
        use hickory_proto::rr::rdata::A;
        use hickory_proto::rr::{RData, Record};

        let name = Name::from_ascii("edns.example.").unwrap();
        let mut msg = Message::new();
        msg.set_id(0x4444);
        msg.set_message_type(MessageType::Response);
        msg.set_response_code(ResponseCode::NoError);
        msg.add_query(Query::query(name.clone(), RecordType::A));
        for i in 1..=40u8 {
            msg.add_answer(Record::from_rdata(
                name.clone(),
                60,
                RData::A(A::new(192, 0, 2, i)),
            ));
        }
        let mut edns = Edns::new();
        edns.set_max_payload(1232);
        msg.set_edns(edns);

        let mut wire = encode_msg(&msg);
        assert!(truncate_udp(&mut wire, Some(512)));
        let parsed = Message::from_vec(&wire).unwrap();
        assert!(parsed.truncated());
        let edns = parsed
            .extensions()
            .as_ref()
            .expect("EDNS OPT should survive truncation");
        assert_eq!(edns.max_payload(), 1232);
        assert!(wire.len() <= 512);
    }

    #[test]
    fn truncate_udp_sets_tc_when_dropping_required_authority_on_nxdomain() {
        use hickory_proto::op::{MessageType, ResponseCode};
        use hickory_proto::rr::rdata::{A, SOA};
        use hickory_proto::rr::{RData, Record};

        let qname = Name::from_ascii("missing.example.").unwrap();
        let zone = Name::from_ascii("example.").unwrap();
        let mname = Name::from_ascii("ns.example.").unwrap();
        let rname = Name::from_ascii("hostmaster.example.").unwrap();
        let mut msg = Message::new();
        msg.set_id(0x5555);
        msg.set_message_type(MessageType::Response);
        msg.set_response_code(ResponseCode::NXDomain);
        msg.add_query(Query::query(qname, RecordType::A));
        msg.add_name_server(Record::from_rdata(
            zone,
            60,
            RData::SOA(SOA::new(mname, rname, 1, 7200, 3600, 1209600, 60)),
        ));
        let glue = Name::from_ascii("pad.example.").unwrap();
        for i in 1..=40u8 {
            msg.add_additional(Record::from_rdata(
                glue.clone(),
                60,
                RData::A(A::new(203, 0, 113, i)),
            ));
        }

        let mut wire = encode_msg(&msg);
        // Force a limit that cannot hold SOA + additionals, and also cannot hold
        // SOA alone after additionals are dropped (very tight).
        let mut soa_only = msg.clone();
        soa_only.additionals_mut().clear();
        let soa_len = encode_msg(&soa_only).len();
        let limit = (soa_len / 2).max(64);
        assert!(limit < soa_len);

        let truncated = truncate_udp(&mut wire, Some(limit as u16));
        assert!(
            truncated,
            "dropping required NXDOMAIN authority must set TC"
        );
        assert!(wire.len() <= limit);
        let parsed = Message::from_vec(&wire).unwrap();
        assert!(parsed.truncated());
        assert!(parsed.answers().is_empty());
    }

    #[test]
    fn truncate_udp_nxdomain_drops_additionals_keeps_soa_without_tc() {
        use hickory_proto::op::{MessageType, ResponseCode};
        use hickory_proto::rr::rdata::{A, SOA};
        use hickory_proto::rr::{RData, Record, RecordType as RT};

        let qname = Name::from_ascii("gone.example.").unwrap();
        let zone = Name::from_ascii("example.").unwrap();
        let mname = Name::from_ascii("ns.example.").unwrap();
        let rname = Name::from_ascii("hostmaster.example.").unwrap();
        let mut msg = Message::new();
        msg.set_id(0x7777);
        msg.set_message_type(MessageType::Response);
        msg.set_response_code(ResponseCode::NXDomain);
        msg.add_query(Query::query(qname, RecordType::A));
        msg.add_name_server(Record::from_rdata(
            zone,
            60,
            RData::SOA(SOA::new(mname, rname, 1, 7200, 3600, 1209600, 60)),
        ));
        let glue = Name::from_ascii("pad.example.").unwrap();
        for i in 1..=40u8 {
            msg.add_additional(Record::from_rdata(
                glue.clone(),
                60,
                RData::A(A::new(203, 0, 113, i)),
            ));
        }

        let mut wire = encode_msg(&msg);
        let mut soa_only = msg.clone();
        soa_only.additionals_mut().clear();
        let soa_len = encode_msg(&soa_only).len();
        let limit = soa_len + 8;
        assert!(limit < wire.len());

        let truncated = truncate_udp(&mut wire, Some(limit as u16));
        assert!(
            !truncated,
            "NXDOMAIN that fits after dropping additionals should keep SOA without TC"
        );
        let parsed = Message::from_vec(&wire).unwrap();
        assert!(!parsed.truncated());
        assert_eq!(parsed.name_servers().len(), 1);
        assert_eq!(parsed.name_servers()[0].record_type(), RT::SOA);
        assert!(parsed.additionals().is_empty());
    }

    #[test]
    fn truncate_udp_positive_answer_drops_authority_without_tc_when_answers_fit() {
        use hickory_proto::op::{MessageType, ResponseCode};
        use hickory_proto::rr::rdata::A;
        use hickory_proto::rr::{RData, Record};

        let qname = Name::from_ascii("www.example.").unwrap();
        let ns = Name::from_ascii("ns.example.").unwrap();
        let mut msg = Message::new();
        msg.set_id(0x6666);
        msg.set_message_type(MessageType::Response);
        msg.set_response_code(ResponseCode::NoError);
        msg.add_query(Query::query(qname.clone(), RecordType::A));
        msg.add_answer(Record::from_rdata(
            qname,
            60,
            RData::A(A::new(192, 0, 2, 9)),
        ));
        for i in 1..=30u8 {
            msg.add_name_server(Record::from_rdata(
                ns.clone(),
                60,
                RData::A(A::new(198, 51, 100, i)),
            ));
        }

        let mut wire = encode_msg(&msg);
        let mut answers_only = msg.clone();
        answers_only.name_servers_mut().clear();
        let answers_len = encode_msg(&answers_only).len();
        let limit = answers_len + 4;
        assert!(limit < wire.len());

        let truncated = truncate_udp(&mut wire, Some(limit as u16));
        assert!(
            !truncated,
            "positive answer that fits after dropping authority should not set TC"
        );
        let parsed = Message::from_vec(&wire).unwrap();
        assert!(!parsed.truncated());
        assert_eq!(parsed.answers().len(), 1);
        assert!(parsed.name_servers().is_empty());
    }
}
