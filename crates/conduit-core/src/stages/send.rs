//! Send phase — ensure response wire is available (built in parse/forward path).

use crate::phase::Phase;
use crate::pipeline::{PipelineStage, StageOutcome};
use crate::snapshot::RuntimeSnapshot;
use crate::transaction::Transaction;
use hickory_proto::op::{Message, ResponseCode};
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
        let wire = build_error_response(txn.dns_id, rcode, txn.client_udp_payload_size);
        txn.response_wire = Some(wire);
        StageOutcome::Continue(Phase::Send)
    }
}

pub fn build_error_response(id: u16, rcode: u16, udp_payload_size: Option<u16>) -> Vec<u8> {
    let mut msg = Message::new();
    msg.set_id(id);
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

    #[test]
    fn error_response_respects_512_without_edns() {
        let wire = build_error_response(42, 2, None);
        assert!(wire.len() <= 512);
        assert_eq!(wire[0], 0);
        assert_eq!(wire[1], 42);
    }
}
