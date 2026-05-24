//! dnstap protobuf decode (top-level Dnstap + nested Message) and DNS enrichment.

use crate::dns::{parse_dns_wire, DnsDetail};
use crate::message_meta::ParsedMessage;
use anyhow::{anyhow, Context, Result};
use serde::Serialize;
use serde_json::Value;

const DNSTAP_TYPE_MESSAGE: u32 = 1;

const FIELD_IDENTITY: u32 = 1;
const FIELD_EXTRA: u32 = 3;
const FIELD_MESSAGE: u32 = 14;
const FIELD_DNSTAP_TYPE: u32 = 15;

#[derive(Debug, Clone, Serialize)]
pub struct DecodedFrame {
    pub dnstap_type: String,
    pub identity: Option<String>,
    pub extra: Option<Value>,
    pub message_type: Option<String>,
    pub mnemonic: Option<String>,
    pub socket_family: Option<String>,
    pub socket_protocol: Option<String>,
    pub query_address: Option<String>,
    pub query_port: Option<u16>,
    pub response_address: Option<String>,
    pub response_port: Option<u16>,
    pub query_time: Option<String>,
    pub response_time: Option<String>,
    pub latency_ms: Option<f64>,
    pub dns_query: Option<DnsDetail>,
    pub dns_response: Option<DnsDetail>,
    /// First question name (convenience; also in dns_query/dns_response).
    pub qname: Option<String>,
    pub wire_len: usize,
}

pub fn decode_dnstap(payload: &[u8]) -> Result<DecodedFrame> {
    if payload.is_empty() {
        return Err(anyhow!("empty dnstap payload"));
    }

    let mut identity = None;
    let mut extra = None;
    let mut message_bytes = None;
    let mut dnstap_type = None;

    for (field, wire, value) in iter_fields(payload)? {
        match field {
            FIELD_IDENTITY if wire == 2 => {
                identity = Some(String::from_utf8_lossy(&value).into_owned());
            }
            FIELD_EXTRA if wire == 2 => {
                extra = Some(parse_extra_json(&value)?);
            }
            FIELD_MESSAGE if wire == 2 => {
                message_bytes = Some(value);
            }
            FIELD_DNSTAP_TYPE if wire == 0 => {
                dnstap_type = Some(read_varint(&value)? as u32);
            }
            _ => {}
        }
    }

    let dnstap_type = match dnstap_type {
        Some(DNSTAP_TYPE_MESSAGE) => "MESSAGE".to_string(),
        Some(n) => format!("UNKNOWN({n})"),
        None => "UNKNOWN".to_string(),
    };

    let mut message_type = None;
    let mut mnemonic = None;
    let mut socket_family = None;
    let mut socket_protocol = None;
    let mut query_address = None;
    let mut query_port = None;
    let mut response_address = None;
    let mut response_port = None;
    let mut query_time = None;
    let mut response_time = None;
    let mut latency_ms = None;
    let mut dns_query = None;
    let mut dns_response = None;
    let mut wire_len = 0usize;

    if let Some(msg) = message_bytes.as_deref() {
        let parsed = ParsedMessage::parse(msg)?;
        message_type = parsed
            .msg_type
            .map(|t| message_type_name(t).to_string());
        mnemonic = parsed.msg_type.map(|t| message_mnemonic(t).to_string());
        socket_family = parsed.socket_family.map(socket_family_name);
        socket_protocol = parsed.socket_protocol.map(socket_protocol_name);
        query_address = parsed
            .query_address
            .as_ref()
            .map(|b| format_ip(b));
        query_port = parsed.query_port;
        response_address = parsed
            .response_address
            .as_ref()
            .map(|b| format_ip(b));
        response_port = parsed.response_port;
        query_time = parsed
            .query_time_sec
            .map(|s| format_timestamp(s, parsed.query_time_nsec.unwrap_or(0)));
        response_time = parsed
            .response_time_sec
            .map(|s| format_timestamp(s, parsed.response_time_nsec.unwrap_or(0)));
        latency_ms = parsed.latency_ms();

        if let Some(wire) = &parsed.query_wire {
            wire_len = wire.len();
            dns_query = parse_dns_wire(wire);
        }
        if let Some(wire) = &parsed.response_wire {
            wire_len = wire.len();
            dns_response = parse_dns_wire(wire);
        }
    }

    let qname = dns_response
        .as_ref()
        .and_then(|d| d.question.as_ref().map(|q| q.name.clone()))
        .or_else(|| {
            dns_query
                .as_ref()
                .and_then(|d| d.question.as_ref().map(|q| q.name.clone()))
        });

    Ok(DecodedFrame {
        dnstap_type,
        identity,
        extra,
        message_type,
        mnemonic,
        socket_family,
        socket_protocol,
        query_address,
        query_port,
        response_address,
        response_port,
        query_time,
        response_time,
        latency_ms,
        dns_query,
        dns_response,
        qname,
        wire_len,
    })
}

fn parse_extra_json(bytes: &[u8]) -> Result<Value> {
    if bytes.is_empty() {
        return Ok(Value::Null);
    }
    serde_json::from_slice(bytes).or_else(|_| {
        Ok(Value::String(String::from_utf8_lossy(bytes).into_owned()))
    })
}

fn format_ip(bytes: &[u8]) -> String {
    match bytes.len() {
        4 => std::net::Ipv4Addr::from(<[u8; 4]>::try_from(bytes).unwrap()).to_string(),
        16 => std::net::Ipv6Addr::from(<[u8; 16]>::try_from(bytes).unwrap()).to_string(),
        _ => format!("0x{}", bytes.iter().map(|b| format!("{b:02x}")).collect::<String>()),
    }
}

fn format_timestamp(sec: u64, nsec: u32) -> String {
    format!("{sec}.{nsec:09}Z")
}

fn socket_family_name(v: u32) -> String {
    match v {
        1 => "INET".into(),
        2 => "INET6".into(),
        n => format!("UNKNOWN({n})"),
    }
}

fn socket_protocol_name(v: u32) -> String {
    match v {
        1 => "UDP".into(),
        2 => "TCP".into(),
        3 => "DOT".into(),
        4 => "DOH".into(),
        5 => "DNSCryptUDP".into(),
        6 => "DNSCryptTCP".into(),
        n => format!("UNKNOWN({n})"),
    }
}

fn message_type_name(t: u32) -> &'static str {
    match t {
        1 => "AUTH_QUERY",
        2 => "AUTH_RESPONSE",
        3 => "RESOLVER_QUERY",
        4 => "RESOLVER_RESPONSE",
        5 => "CLIENT_QUERY",
        6 => "CLIENT_RESPONSE",
        7 => "FORWARDER_QUERY",
        8 => "FORWARDER_RESPONSE",
        9 => "STUB_QUERY",
        10 => "STUB_RESPONSE",
        11 => "TOOL_QUERY",
        12 => "TOOL_RESPONSE",
        _ => "UNKNOWN",
    }
}

fn message_mnemonic(t: u32) -> &'static str {
    match t {
        1 => "AQ",
        2 => "AR",
        3 => "RQ",
        4 => "RR",
        5 => "CQ",
        6 => "CR",
        7 => "FQ",
        8 => "FR",
        9 => "SQ",
        10 => "SR",
        11 => "TQ",
        12 => "TR",
        _ => "??",
    }
}

/// Collected field: (field number, wire type, value bytes).
pub(crate) fn iter_fields(bytes: &[u8]) -> Result<Vec<(u32, u8, Vec<u8>)>> {
    let mut out = Vec::new();
    let mut pos = 0usize;
    while pos < bytes.len() {
        let (tag, new_pos) = read_varint_at(bytes, pos).context("field tag")?;
        pos = new_pos;
        let field = (tag >> 3) as u32;
        let wire = (tag & 0x7) as u8;
        match wire {
            0 => {
                let (val, new_pos) = read_varint_at(bytes, pos).context("varint field")?;
                pos = new_pos;
                let mut buf = Vec::new();
                write_varint(&mut buf, val);
                out.push((field, wire, buf));
            }
            1 => {
                pos = skip_bytes(bytes, pos, 8)?;
            }
            2 => {
                let (len, new_pos) = read_varint_at(bytes, pos).context("length-delimited")?;
                pos = new_pos;
                let end = pos
                    .checked_add(len as usize)
                    .ok_or_else(|| anyhow!("length overflow"))?;
                if end > bytes.len() {
                    return Err(anyhow!("truncated protobuf"));
                }
                out.push((field, wire, bytes[pos..end].to_vec()));
                pos = end;
            }
            5 => {
                pos = skip_bytes(bytes, pos, 4)?;
            }
            3 | 4 => return Err(anyhow!("deprecated protobuf group encoding")),
            _ => return Err(anyhow!("unknown wire type {wire}")),
        }
    }
    Ok(out)
}

pub(crate) fn skip_bytes(bytes: &[u8], pos: usize, len: usize) -> Result<usize> {
    let end = pos
        .checked_add(len)
        .ok_or_else(|| anyhow!("length overflow"))?;
    if end > bytes.len() {
        return Err(anyhow!("truncated protobuf"));
    }
    Ok(end)
}

pub(crate) fn read_varint(data: &[u8]) -> Result<u64> {
    read_varint_at(data, 0).map(|(n, _)| n)
}

pub(crate) fn read_varint_at(data: &[u8], mut pos: usize) -> Result<(u64, usize)> {
    let mut result = 0u64;
    let mut shift = 0;
    while pos < data.len() {
        let byte = data[pos];
        pos += 1;
        result |= ((byte & 0x7f) as u64) << shift;
        if byte & 0x80 == 0 {
            return Ok((result, pos));
        }
        shift += 7;
        if shift > 63 {
            return Err(anyhow!("varint overflow"));
        }
    }
    Err(anyhow!("truncated varint"))
}

pub(crate) fn write_varint(out: &mut Vec<u8>, mut n: u64) {
    while n >= 0x80 {
        out.push((n as u8) | 0x80);
        n >>= 7;
    }
    out.push(n as u8);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decode_skips_fixed32_nsec_fields() {
        let mut msg = Vec::new();
        write_tag(&mut msg, 1, 0);
        write_varint(&mut msg, 6);
        write_tag(&mut msg, 9, 5);
        msg.extend_from_slice(&[0x10, 0x27, 0x00, 0x00]);
        write_tag(&mut msg, 10, 2);
        write_bytes(&mut msg, &[0x00, 0x00]);

        let mut dnstap = Vec::new();
        write_tag(&mut dnstap, 3, 2);
        write_bytes(&mut dnstap, br#"{"pool":"p1"}"#);
        write_tag(&mut dnstap, 15, 0);
        write_varint(&mut dnstap, 1);
        write_tag(&mut dnstap, 14, 2);
        write_bytes(&mut dnstap, &msg);

        let frame = decode_dnstap(&dnstap).unwrap();
        assert_eq!(frame.mnemonic.as_deref(), Some("CR"));
        assert_eq!(frame.extra.as_ref().and_then(|v| v.get("pool")).unwrap(), "p1");
        assert!(frame.query_time.is_none());
        assert!(frame.response_time.is_none());
    }

    fn write_tag(out: &mut Vec<u8>, field: u32, wire: u8) {
        write_varint(out, ((field << 3) | u32::from(wire)) as u64);
    }

    fn write_bytes(out: &mut Vec<u8>, data: &[u8]) {
        write_varint(out, data.len() as u64);
        out.extend_from_slice(data);
    }
}
