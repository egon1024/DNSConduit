//! DNS cache key construction (full-key equality; hash routes shard only).

use crate::transaction::Transaction;
use hickory_proto::error::ProtoError;
use hickory_proto::op::Message;
use hickory_proto::rr::Name;
use hickory_proto::serialize::binary::{BinEncodable, BinEncoder};

const KEY_VERSION: u8 = 1;

/// Answer-shape dimension in the cache key (not client transport).
///
/// Complete answers are shared across UDP and TCP clients. Truncated UDP
/// stubs (`truncated_udp`) use a distinct key and are never served to TCP.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum TransportKey {
    /// Complete (TC=0) answer — shared by UDP and TCP clients.
    /// Byte value `0` matches the former UDP-only complete key so live
    /// in-memory UDP entries remain hittable after upgrade without restart.
    Complete = 0,
    /// Stored TC=1 UDP answers when `truncated_udp.enabled` is true.
    /// Kept at `2` so leftover pre-unification TCP keys (`1`) are not
    /// mistaken for truncated stubs.
    UdpTruncated = 2,
}

impl TransportKey {
    pub fn as_byte(self) -> u8 {
        self as u8
    }
}

/// Canonical cache key bytes (variable length).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CacheKey(pub Vec<u8>);

impl CacheKey {
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }
}

/// Build the lookup key for a complete (non-truncated) cached answer.
///
/// Client transport (UDP vs TCP) is not part of the key — both share this
/// shape. UDP Send still truncates oversized wire to the client's EDNS
/// payload size (or 512) when serving.
pub fn build_query_key(txn: &Transaction) -> Result<CacheKey, ProtoError> {
    build_key_from_parts(
        txn.qname.as_deref().unwrap_or("."),
        txn.qtype.unwrap_or(0),
        txn.qclass.unwrap_or(1),
        &txn.query_wire,
        TransportKey::Complete,
    )
}

/// Build a key for storing a truncated UDP answer.
pub fn build_truncated_udp_key(txn: &Transaction) -> Result<CacheKey, ProtoError> {
    build_key_from_parts(
        txn.qname.as_deref().unwrap_or("."),
        txn.qtype.unwrap_or(0),
        txn.qclass.unwrap_or(1),
        &txn.query_wire,
        TransportKey::UdpTruncated,
    )
}

/// Build a cache key from normalized query parts (used for exact and ancestor lookups).
pub(crate) fn build_key_from_parts(
    qname: &str,
    qtype: u16,
    qclass: u16,
    query_wire: &[u8],
    transport: TransportKey,
) -> Result<CacheKey, ProtoError> {
    let (cd, do_bit) = dnssec_flags(query_wire)?;
    let ecs = ecs_option_bytes(query_wire)?;

    let normalized = normalize_qname(qname)?;
    let mut out = Vec::with_capacity(normalized.len() + ecs.len() + 16);
    out.push(KEY_VERSION);
    out.extend_from_slice(&(normalized.len() as u16).to_be_bytes());
    out.extend_from_slice(&normalized);
    out.extend_from_slice(&qtype.to_be_bytes());
    out.extend_from_slice(&qclass.to_be_bytes());
    let flags = (u8::from(cd) << 1) | u8::from(do_bit);
    out.push(flags);
    out.push(transport.as_byte());
    out.push(ecs.len() as u8);
    out.extend_from_slice(&ecs);
    Ok(CacheKey(out))
}

fn normalize_qname(qname: &str) -> Result<Vec<u8>, ProtoError> {
    let trimmed = qname.trim_end_matches('.');
    let lower = trimmed.to_ascii_lowercase();
    let fqdn = if lower.is_empty() {
        ".".to_string()
    } else if qname.ends_with('.') {
        format!("{lower}.")
    } else {
        lower
    };
    let name = Name::from_utf8(fqdn)?;
    let mut buf = Vec::new();
    let mut enc = BinEncoder::new(&mut buf);
    name.emit(&mut enc)?;
    Ok(buf)
}

fn dnssec_flags(query_wire: &[u8]) -> Result<(bool, bool), ProtoError> {
    let msg = Message::from_vec(query_wire)?;
    let cd = msg.checking_disabled();
    let do_bit = msg
        .extensions()
        .as_ref()
        .map(|e| e.dnssec_ok())
        .unwrap_or(false);
    Ok((cd, do_bit))
}

fn ecs_option_bytes(query_wire: &[u8]) -> Result<Vec<u8>, ProtoError> {
    let msg = Message::from_vec(query_wire)?;
    let Some(edns) = msg.extensions() else {
        return Ok(Vec::new());
    };
    use hickory_proto::rr::rdata::opt::EdnsOption;
    for opt in edns.options().as_ref().values() {
        if let EdnsOption::Subnet(subnet) = opt {
            return Vec::try_from(subnet);
        }
    }
    Ok(Vec::new())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transaction::{ClientProtocol, Transaction};
    use hickory_proto::op::{Message, Query};
    use hickory_proto::rr::{Name, RecordType};
    use hickory_proto::serialize::binary::{BinEncodable, BinEncoder};
    use std::net::SocketAddr;

    fn example_query() -> Vec<u8> {
        let name = Name::from_utf8("www.example.com.").unwrap();
        let mut msg = Message::new();
        msg.add_query(Query::query(name, RecordType::A));
        let mut buf = Vec::new();
        let mut enc = BinEncoder::new(&mut buf);
        msg.emit(&mut enc).unwrap();
        buf
    }

    #[test]
    fn distinct_qnames_produce_distinct_keys() {
        let addr: SocketAddr = "127.0.0.1:53".parse().unwrap();
        let wire = example_query();

        let mut a = Transaction::new(1, addr, ClientProtocol::Udp);
        a.qname = Some("www.example.com.".into());
        a.qtype = Some(1);
        a.qclass = Some(1);
        a.query_wire = wire.clone();

        let mut b = Transaction::new(2, addr, ClientProtocol::Udp);
        b.qname = Some("other.example.com.".into());
        b.qtype = Some(1);
        b.qclass = Some(1);
        b.query_wire = wire;

        let ka = build_query_key(&a).unwrap();
        let kb = build_query_key(&b).unwrap();
        assert_ne!(ka, kb);
    }

    #[test]
    fn case_insensitive_qname_normalization() {
        let addr: SocketAddr = "127.0.0.1:53".parse().unwrap();
        let wire = example_query();

        let mut lower = Transaction::new(1, addr, ClientProtocol::Udp);
        lower.qname = Some("www.example.com.".into());
        lower.qtype = Some(1);
        lower.qclass = Some(1);
        lower.query_wire = wire.clone();

        let mut upper = Transaction::new(2, addr, ClientProtocol::Udp);
        upper.qname = Some("WWW.EXAMPLE.COM.".into());
        upper.qtype = Some(1);
        upper.qclass = Some(1);
        upper.query_wire = wire;

        assert_eq!(
            build_query_key(&lower).unwrap(),
            build_query_key(&upper).unwrap()
        );
    }

    #[test]
    fn udp_and_tcp_clients_share_complete_answer_key() {
        let addr: SocketAddr = "127.0.0.1:53".parse().unwrap();
        let wire = example_query();

        let mut udp = Transaction::new(1, addr, ClientProtocol::Udp);
        udp.qname = Some("www.example.com.".into());
        udp.qtype = Some(1);
        udp.qclass = Some(1);
        udp.query_wire = wire.clone();

        let mut tcp = Transaction::new(2, addr, ClientProtocol::Tcp);
        tcp.qname = Some("www.example.com.".into());
        tcp.qtype = Some(1);
        tcp.qclass = Some(1);
        tcp.query_wire = wire;

        assert_eq!(
            build_query_key(&udp).unwrap(),
            build_query_key(&tcp).unwrap()
        );
    }

    #[test]
    fn truncated_udp_key_differs_from_complete_key() {
        let addr: SocketAddr = "127.0.0.1:53".parse().unwrap();
        let wire = example_query();

        let mut txn = Transaction::new(1, addr, ClientProtocol::Udp);
        txn.qname = Some("www.example.com.".into());
        txn.qtype = Some(1);
        txn.qclass = Some(1);
        txn.query_wire = wire;

        assert_ne!(
            build_query_key(&txn).unwrap(),
            build_truncated_udp_key(&txn).unwrap()
        );
    }

    fn query_with_dnssec_flags(cd: bool, do_bit: bool) -> Vec<u8> {
        use hickory_proto::op::Edns;
        let name = Name::from_utf8("www.example.com.").unwrap();
        let mut msg = Message::new();
        msg.add_query(Query::query(name, RecordType::A));
        msg.set_checking_disabled(cd);
        let mut edns = Edns::new();
        edns.set_max_payload(1232);
        edns.set_dnssec_ok(do_bit);
        msg.set_edns(edns);
        let mut buf = Vec::new();
        let mut enc = BinEncoder::new(&mut buf);
        msg.emit(&mut enc).unwrap();
        buf
    }

    fn txn_with_wire(id: u64, wire: Vec<u8>) -> Transaction {
        let addr: SocketAddr = "127.0.0.1:53".parse().unwrap();
        let mut txn = Transaction::new(id, addr, ClientProtocol::Udp);
        txn.qname = Some("www.example.com.".into());
        txn.qtype = Some(1);
        txn.qclass = Some(1);
        txn.query_wire = wire;
        txn
    }

    #[test]
    fn cd_and_do_bits_produce_distinct_keys() {
        let neither = txn_with_wire(1, query_with_dnssec_flags(false, false));
        let cd_only = txn_with_wire(2, query_with_dnssec_flags(true, false));
        let do_only = txn_with_wire(3, query_with_dnssec_flags(false, true));
        let both = txn_with_wire(4, query_with_dnssec_flags(true, true));

        let k_neither = build_query_key(&neither).unwrap();
        let k_cd = build_query_key(&cd_only).unwrap();
        let k_do = build_query_key(&do_only).unwrap();
        let k_both = build_query_key(&both).unwrap();

        assert_ne!(k_neither, k_cd, "CD bit must change the cache key");
        assert_ne!(k_neither, k_do, "DO bit must change the cache key");
        assert_ne!(k_cd, k_do, "CD-only and DO-only keys must differ");
        assert_ne!(k_both, k_cd);
        assert_ne!(k_both, k_do);
        assert_ne!(k_both, k_neither);
    }
}
