//! Parse dnstap.Message protobuf fields.

use crate::decode::{read_varint_at, skip_bytes};
use anyhow::{anyhow, Result};

const MSG_TYPE: u32 = 1;
const MSG_SOCKET_FAMILY: u32 = 2;
const MSG_SOCKET_PROTOCOL: u32 = 3;
const MSG_QUERY_ADDRESS: u32 = 4;
const MSG_RESPONSE_ADDRESS: u32 = 5;
const MSG_QUERY_PORT: u32 = 6;
const MSG_RESPONSE_PORT: u32 = 7;
const MSG_QUERY_TIME_SEC: u32 = 8;
const MSG_QUERY_TIME_NSEC: u32 = 9;
const MSG_QUERY_MESSAGE: u32 = 10;
const MSG_RESPONSE_TIME_SEC: u32 = 12;
const MSG_RESPONSE_TIME_NSEC: u32 = 13;
const MSG_RESPONSE_MESSAGE: u32 = 14;

#[derive(Debug, Clone, Default)]
pub struct ParsedMessage {
    pub msg_type: Option<u32>,
    pub socket_family: Option<u32>,
    pub socket_protocol: Option<u32>,
    pub query_address: Option<Vec<u8>>,
    pub response_address: Option<Vec<u8>>,
    pub query_port: Option<u16>,
    pub response_port: Option<u16>,
    pub query_time_sec: Option<u64>,
    pub query_time_nsec: Option<u32>,
    pub response_time_sec: Option<u64>,
    pub response_time_nsec: Option<u32>,
    pub query_wire: Option<Vec<u8>>,
    pub response_wire: Option<Vec<u8>>,
}

impl ParsedMessage {
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        let mut m = Self::default();
        let mut pos = 0usize;
        while pos < bytes.len() {
            let (tag, new_pos) = read_varint_at(bytes, pos)?;
            pos = new_pos;
            let field = (tag >> 3) as u32;
            let wire = (tag & 0x7) as u8;
            match wire {
                0 => {
                    let (val, new_pos) = read_varint_at(bytes, pos)?;
                    pos = new_pos;
                    match field {
                        MSG_TYPE => m.msg_type = Some(val as u32),
                        MSG_SOCKET_FAMILY => m.socket_family = Some(val as u32),
                        MSG_SOCKET_PROTOCOL => m.socket_protocol = Some(val as u32),
                        MSG_QUERY_PORT => m.query_port = Some(val as u16),
                        MSG_RESPONSE_PORT => m.response_port = Some(val as u16),
                        MSG_QUERY_TIME_SEC => m.query_time_sec = Some(val),
                        MSG_RESPONSE_TIME_SEC => m.response_time_sec = Some(val),
                        _ => {}
                    }
                }
                1 => {
                    pos = skip_bytes(bytes, pos, 8)?;
                }
                2 => {
                    let (len, new_pos) = read_varint_at(bytes, pos)?;
                    pos = new_pos;
                    let end = pos
                        .checked_add(len as usize)
                        .ok_or_else(|| anyhow!("length overflow"))?;
                    if end > bytes.len() {
                        return Err(anyhow!("truncated protobuf"));
                    }
                    let val = bytes[pos..end].to_vec();
                    match field {
                        MSG_QUERY_ADDRESS => m.query_address = Some(val),
                        MSG_RESPONSE_ADDRESS => m.response_address = Some(val),
                        MSG_QUERY_MESSAGE => m.query_wire = Some(val),
                        MSG_RESPONSE_MESSAGE => m.response_wire = Some(val),
                        _ => {}
                    }
                    pos = end;
                }
                5 => {
                    if pos + 4 > bytes.len() {
                        return Err(anyhow!("truncated fixed32"));
                    }
                    let val = u32::from_le_bytes(bytes[pos..pos + 4].try_into().unwrap());
                    pos += 4;
                    match field {
                        MSG_QUERY_TIME_NSEC => m.query_time_nsec = Some(val),
                        MSG_RESPONSE_TIME_NSEC => m.response_time_nsec = Some(val),
                        _ => {}
                    }
                }
                3 | 4 => return Err(anyhow!("deprecated protobuf group encoding")),
                _ => return Err(anyhow!("unknown wire type {wire}")),
            }
        }
        Ok(m)
    }

    pub fn latency_ms(&self) -> Option<f64> {
        let (qs, qn, rs, rn) = (
            self.query_time_sec?,
            self.query_time_nsec.unwrap_or(0),
            self.response_time_sec?,
            self.response_time_nsec.unwrap_or(0),
        );
        let q = qs as f64 + qn as f64 / 1e9;
        let r = rs as f64 + rn as f64 / 1e9;
        Some((r - q) * 1000.0)
    }
}
