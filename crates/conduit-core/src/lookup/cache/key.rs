//! DNS cache key construction (full-key equality; hash routes shard only).

use crate::transaction::{ClientProtocol, Transaction};
use hickory_proto::error::ProtoError;
use hickory_proto::op::Message;
use hickory_proto::rr::Name;
use hickory_proto::serialize::binary::{BinEncodable, BinEncoder};

const KEY_VERSION: u8 = 1;

/// Transport dimension in the cache key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum TransportKey {
    Udp = 0,
    Tcp = 1,
    /// Stored TC=1 UDP answers when `cache_truncated_udp` is enabled.
    UdpTruncated = 2,
}

impl TransportKey {
    pub fn from_client(protocol: ClientProtocol) -> Self {
        match protocol {
            ClientProtocol::Udp => Self::Udp,
            ClientProtocol::Tcp => Self::Tcp,
        }
    }

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

/// Build the lookup key for a client query.
pub fn build_query_key(txn: &Transaction) -> Result<CacheKey, ProtoError> {
    build_key_from_parts(
        txn.qname.as_deref().unwrap_or("."),
        txn.qtype.unwrap_or(0),
        txn.qclass.unwrap_or(1),
        &txn.query_wire,
        TransportKey::from_client(txn.protocol),
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

fn build_key_from_parts(
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
}
