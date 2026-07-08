//! Prepare cached wire for client response (ID rewrite, TTL decay, optional RRset rotation).

use hickory_proto::error::ProtoError;
use hickory_proto::op::Message;
use hickory_proto::rr::Record;
use hickory_proto::serialize::binary::{BinEncodable, BinEncoder};
use std::cell::Cell;
use std::sync::Arc;
use std::time::Instant;

thread_local! {
    /// Fast thread-local PRNG for RRset rotation offsets (no external crate on hot path).
    static SERVE_RAND: Cell<u64> = const { Cell::new(0x9E37_79B9_7F4A_7C15) };
}

fn next_serve_rand() -> u64 {
    SERVE_RAND.with(|state| {
        let mut x = state.get();
        // xorshift64*
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        x = x.wrapping_mul(0x2545_F491_4F6C_DD1D);
        state.set(x);
        x
    })
}

/// Clone stored wire, set response ID, decay RR TTLs by cache age, optionally rotate answer RRsets.
/// When rotation is enabled, each serve picks a random cyclic offset per RRset (stored wire unchanged).
/// When `client_query_wire` is set, replace the question (and EDNS) from the client query — used
/// for RFC 8020 ancestor NXDOMAIN hits where the stored wire names the parent qname.
pub fn prepare_served_wire(
    stored: &[u8],
    query_id: u16,
    rotate_rrset: bool,
    filled_at: Instant,
    now: Instant,
    client_query_wire: Option<&[u8]>,
) -> Result<Vec<u8>, ProtoError> {
    let mut msg = Message::from_vec(stored)?;
    msg.set_id(query_id);
    if let Some(qw) = client_query_wire {
        apply_client_query(&mut msg, qw)?;
    }
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

fn apply_client_query(msg: &mut Message, client_query_wire: &[u8]) -> Result<(), ProtoError> {
    let client = Message::from_vec(client_query_wire)?;
    msg.queries_mut().clear();
    for q in client.queries() {
        msg.add_query(q.clone());
    }
    if let Some(edns) = client.extensions().clone() {
        msg.set_edns(edns);
    }
    Ok(())
}

pub fn prepare_served_arc(
    stored: &Arc<[u8]>,
    query_id: u16,
    rotate_rrset: bool,
    filled_at: Instant,
    now: Instant,
    client_query_wire: Option<&[u8]>,
) -> Result<Vec<u8>, ProtoError> {
    prepare_served_wire(
        stored,
        query_id,
        rotate_rrset,
        filled_at,
        now,
        client_query_wire,
    )
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
            let offset = (next_serve_rand() as usize) % rrs.len();
            rrs.rotate_left(offset);
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

    fn three_a_records() -> Arc<[u8]> {
        encode_response(vec![
            (60, std::net::Ipv4Addr::new(1, 1, 1, 1)),
            (60, std::net::Ipv4Addr::new(2, 2, 2, 2)),
            (60, std::net::Ipv4Addr::new(3, 3, 3, 3)),
        ])
    }

    #[test]
    fn rotate_differs_from_plain() {
        let stored = two_a_records();
        let now = Instant::now();
        let plain = prepare_served_wire(&stored, 42, false, now, now, None).unwrap();
        let mut saw_different = false;
        for i in 0..16 {
            let rotated = prepare_served_wire(&stored, i as u16, true, now, now, None).unwrap();
            if rotated != plain {
                saw_different = true;
                break;
            }
        }
        assert!(
            saw_different,
            "expected at least one rotated serve to differ from plain"
        );
    }

    #[test]
    fn rotate_varies_across_serves() {
        let stored = three_a_records();
        let now = Instant::now();
        let mut prev: Option<Vec<u8>> = None;
        let mut all_same = true;
        for i in 0..24 {
            let out = prepare_served_wire(&stored, i as u16, true, now, now, None).unwrap();
            if let Some(p) = &prev {
                if p != &out {
                    all_same = false;
                    break;
                }
            }
            prev = Some(out);
        }
        assert!(
            !all_same,
            "expected RRset rotation to vary across repeated cache serves"
        );
    }

    #[test]
    fn rewrites_query_id() {
        let stored = two_a_records();
        let now = Instant::now();
        let out = prepare_served_wire(&stored, 0xabcd, false, now, now, None).unwrap();
        let msg = Message::from_vec(&out).unwrap();
        assert_eq!(msg.id(), 0xabcd);
    }

    #[test]
    fn decays_answer_ttl_by_cache_age() {
        let stored = encode_response(vec![(3600, std::net::Ipv4Addr::new(192, 0, 2, 50))]);
        let filled_at = Instant::now() - Duration::from_secs(120);
        let now = Instant::now();
        let out = prepare_served_wire(&stored, 1, false, filled_at, now, None).unwrap();
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
        let out = prepare_served_wire(&stored, 1, false, filled_at, now, None).unwrap();
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
        let _ = prepare_served_wire(&stored, 1, false, filled_at, now, None).unwrap();
        let msg = Message::from_vec(&stored).unwrap();
        assert_eq!(msg.answers()[0].ttl(), 3600);
    }
}
