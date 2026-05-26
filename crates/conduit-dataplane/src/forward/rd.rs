//! RD (recursion desired) bit policy on upstream query wire.

use conduit_config::forward::RecursionDesired;
use hickory_proto::op::Message;
use hickory_proto::serialize::binary::BinEncodable;

/// Build upstream wire with RD policy applied. Does not mutate `query_wire`.
pub fn build_upstream_wire(query_wire: &[u8], policy: RecursionDesired) -> Vec<u8> {
    if policy == RecursionDesired::Preserve {
        return query_wire.to_vec();
    }
    let Ok(mut msg) = Message::from_vec(query_wire) else {
        tracing::debug!("forward: failed to parse query for RD policy; preserving wire");
        return query_wire.to_vec();
    };
    let mut header = *msg.header();
    header.set_recursion_desired(policy == RecursionDesired::Set);
    msg.set_header(header);
    let mut out = Vec::new();
    let mut encoder = hickory_proto::serialize::binary::BinEncoder::new(&mut out);
    if msg.emit(&mut encoder).is_err() {
        tracing::debug!("forward: failed to encode query for RD policy; preserving wire");
        return query_wire.to_vec();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use hickory_proto::op::{Message, Query};
    use hickory_proto::rr::{Name, RecordType};
    use hickory_proto::serialize::binary::{BinEncodable, BinEncoder};

    fn sample_query(rd: bool) -> Vec<u8> {
        let name = Name::from_utf8("example.com.").unwrap();
        let query = Query::query(name, RecordType::A);
        let mut msg = Message::new();
        let mut header = *msg.header();
        header.set_id(0x1234);
        header.set_recursion_desired(rd);
        msg.set_header(header);
        msg.add_query(query);
        let mut out = Vec::new();
        let mut enc = BinEncoder::new(&mut out);
        msg.emit(&mut enc).unwrap();
        out
    }

    fn rd_flag(wire: &[u8]) -> bool {
        Message::from_vec(wire)
            .unwrap()
            .header()
            .recursion_desired()
    }

    #[test]
    fn preserve_keeps_rd() {
        let q = sample_query(true);
        let out = build_upstream_wire(&q, RecursionDesired::Preserve);
        assert!(rd_flag(&out));
    }

    #[test]
    fn clear_zeros_rd() {
        let q = sample_query(true);
        let out = build_upstream_wire(&q, RecursionDesired::Clear);
        assert!(!rd_flag(&out));
    }

    #[test]
    fn set_forces_rd() {
        let q = sample_query(false);
        let out = build_upstream_wire(&q, RecursionDesired::Set);
        assert!(rd_flag(&out));
    }
}
