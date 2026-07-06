//! Prepare cached wire for client response (ID rewrite, TTL decay, optional RRset rotation).

use hickory_proto::error::ProtoError;
use hickory_proto::op::Message;
use hickory_proto::rr::Record;
use hickory_proto::serialize::binary::{BinEncodable, BinEncoder};
use std::sync::Arc;
use std::time::Instant;

/// Clone stored wire, set response ID, decay RR TTLs by cache age, optionally rotate answer RRsets.
pub fn prepare_served_wire(
    stored: &[u8],
    query_id: u16,
    rotate_rrset: bool,
    filled_at: Instant,
    now: Instant,
) -> Result<Vec<u8>, ProtoError> {
    let mut msg = Message::from_vec(stored)?;
    msg.set_id(query_id);
    let age_secs = cache_age_secs(filled_at, now);
    decay_rr_ttls(&mut msg, age_secs);
    if rotate_rrset {
        rotate_answer_rrsets(&mut msg);
    }
    let mut buf = Vec::new();
    let mut enc = BinEncoder::new(&mut buf);
    msg.emit(&mut enc)?;
    Ok(buf)
}

pub fn prepare_served_arc(
    stored: &Arc<[u8]>,
    query_id: u16,
    rotate_rrset: bool,
    filled_at: Instant,
    now: Instant,
) -> Result<Vec<u8>, ProtoError> {
    prepare_served_wire(stored, query_id, rotate_rrset, filled_at, now)
}

/// Whole seconds since the entry was stored (DNS TTL aging uses second granularity).
pub fn cache_age_secs(filled_at: Instant, now: Instant) -> u32 {
    now.saturating_duration_since(filled_at)
        .as_secs()
        .min(u32::MAX as u64) as u32
}

fn decay_rr_ttls(msg: &mut Message, age_secs: u32) {
    if age_secs == 0 {
        return;
    }
    decay_section(msg.answers_mut(), age_secs);
    decay_section(msg.name_servers_mut(), age_secs);
    decay_section(msg.additionals_mut(), age_secs);
}

fn decay_section(section: &mut [Record], age_secs: u32) {
    for rr in section.iter_mut() {
        rr.set_ttl(rr.ttl().saturating_sub(age_secs));
    }
}

fn rotate_answer_rrsets(msg: &mut Message) {
    let answers = msg.answers_mut();
    if answers.len() <= 1 {
        return;
    }
    let mut groups: Vec<(String, u16, Vec<Record>)> = Vec::new();
    for rr in answers.drain(..) {
        let name = rr.name().to_utf8();
        let rtype = u16::from(rr.record_type());
        if let Some(g) = groups
            .iter_mut()
            .find(|(n, t, _)| *n == name && *t == rtype)
        {
            g.2.push(rr);
        } else {
            groups.push((name, rtype, vec![rr]));
        }
    }
    for (_, _, mut rrs) in groups {
        if rrs.len() > 1 {
            // Rotate: move first to end (deterministic permute per hit).
            let first = rrs.remove(0);
            rrs.push(first);
        }
        for rr in rrs {
            answers.push(rr);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hickory_proto::op::{Message, Query};
    use hickory_proto::rr::{Name, RData, Record, RecordType};
    use hickory_proto::serialize::binary::{BinEncodable, BinEncoder};
    use std::time::Duration;

    fn encode_response(answers: Vec<(u32, std::net::Ipv4Addr)>) -> Arc<[u8]> {
        let name = Name::from_utf8("example.com.").unwrap();
        let mut msg = Message::new();
        msg.add_query(Query::query(name.clone(), RecordType::A));
        for (ttl, addr) in answers {
            msg.add_answer(Record::from_rdata(
                name.clone(),
                ttl,
                RData::A(hickory_proto::rr::rdata::A(addr)),
            ));
        }
        let mut buf = Vec::new();
        let mut enc = BinEncoder::new(&mut buf);
        msg.emit(&mut enc).unwrap();
        buf.into()
    }

    fn two_a_records() -> Arc<[u8]> {
        encode_response(vec![
            (60, std::net::Ipv4Addr::new(1, 1, 1, 1)),
            (60, std::net::Ipv4Addr::new(2, 2, 2, 2)),
        ])
    }

    #[test]
    fn rotate_changes_order() {
        let stored = two_a_records();
        let now = Instant::now();
        let plain = prepare_served_wire(&stored, 42, false, now, now).unwrap();
        let rotated = prepare_served_wire(&stored, 42, true, now, now).unwrap();
        assert_ne!(plain, rotated);
    }

    #[test]
    fn rewrites_query_id() {
        let stored = two_a_records();
        let now = Instant::now();
        let out = prepare_served_wire(&stored, 0xabcd, false, now, now).unwrap();
        let msg = Message::from_vec(&out).unwrap();
        assert_eq!(msg.id(), 0xabcd);
    }

    #[test]
    fn decays_answer_ttl_by_cache_age() {
        let stored = encode_response(vec![(3600, std::net::Ipv4Addr::new(192, 0, 2, 50))]);
        let filled_at = Instant::now() - Duration::from_secs(120);
        let now = Instant::now();
        let out = prepare_served_wire(&stored, 1, false, filled_at, now).unwrap();
        let msg = Message::from_vec(&out).unwrap();
        let ttl = msg.answers()[0].ttl();
        assert!(ttl <= 3480, "expected decayed TTL, got {ttl}");
        assert!(ttl >= 3478, "expected ~3480s remaining, got {ttl}");
    }

    #[test]
    fn decays_each_rr_independently() {
        let stored = encode_response(vec![
            (300, std::net::Ipv4Addr::new(1, 1, 1, 1)),
            (60, std::net::Ipv4Addr::new(2, 2, 2, 2)),
        ]);
        let filled_at = Instant::now() - Duration::from_secs(30);
        let now = Instant::now();
        let out = prepare_served_wire(&stored, 1, false, filled_at, now).unwrap();
        let msg = Message::from_vec(&out).unwrap();
        let answers = msg.answers();
        assert_eq!(answers[0].ttl(), 270);
        assert_eq!(answers[1].ttl(), 30);
    }

    #[test]
    fn stored_wire_ttl_unchanged() {
        let stored = encode_response(vec![(3600, std::net::Ipv4Addr::new(192, 0, 2, 1))]);
        let filled_at = Instant::now() - Duration::from_secs(60);
        let now = Instant::now();
        let _ = prepare_served_wire(&stored, 1, false, filled_at, now).unwrap();
        let msg = Message::from_vec(&stored).unwrap();
        assert_eq!(msg.answers()[0].ttl(), 3600);
    }
}
