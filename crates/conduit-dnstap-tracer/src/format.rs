//! stdout formatters: log line, JSON (NDJSON), YAML.

use crate::decode::DecodedFrame;
use anyhow::Result;
use std::io::Write;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputFormat {
    Log,
    Json,
    Yaml,
}

impl OutputFormat {
    pub fn parse(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "log" | "line" => Some(Self::Log),
            "json" => Some(Self::Json),
            "yaml" | "yml" => Some(Self::Yaml),
            _ => None,
        }
    }
}

pub fn write_frame(out: &mut impl Write, format: OutputFormat, frame: &DecodedFrame) -> Result<()> {
    match format {
        OutputFormat::Log => write_log(out, frame),
        OutputFormat::Json => write_json(out, frame),
        OutputFormat::Yaml => write_yaml(out, frame),
    }
}

fn write_log(out: &mut impl Write, f: &DecodedFrame) -> Result<()> {
    let mnemonic = f.mnemonic.as_deref().unwrap_or("??");
    let qname = f.qname.as_deref().unwrap_or("-");
    let identity = f.identity.as_deref().unwrap_or("-");
    let extra = f
        .extra
        .as_ref()
        .map(|v| v.to_string())
        .unwrap_or_else(|| "-".to_string());

    writeln!(
        out,
        "{mnemonic} id={} qname={qname} identity={identity} proto={} client={}:{}",
        f.dns_response
            .as_ref()
            .or(f.dns_query.as_ref())
            .map(|d| d.header.id)
            .unwrap_or(0),
        f.socket_protocol.as_deref().unwrap_or("-"),
        f.query_address.as_deref().unwrap_or("-"),
        f.query_port
            .map(|p| p.to_string())
            .unwrap_or_else(|| "-".into()),
    )?;

    if let Some(d) = f.dns_response.as_ref().or(f.dns_query.as_ref()) {
        if let Some(q) = &d.question {
            writeln!(out, "  question: {} {} {}", q.name, q.qtype, q.qclass)?;
        }
        let h = &d.header;
        writeln!(
            out,
            "  header: opcode={} rcode={} flags=qr={},aa={},tc={},rd={},ra={},ad={},cd={} counts={}/{}/{}/{}",
            h.opcode,
            h.rcode.as_deref().unwrap_or("-"),
            h.qr, h.aa, h.tc, h.rd, h.ra, h.ad, h.cd,
            h.query_count, h.answer_count, h.authority_count, h.additional_count,
        )?;
        write_rr_section(out, "  answers", &d.answers)?;
        write_rr_section(out, "  authority", &d.authority)?;
        write_rr_section(out, "  additional", &d.additional)?;
    }

    if f.response_time.is_some() || f.query_time.is_some() {
        writeln!(
            out,
            "  time: query={} response={} latency_ms={}",
            f.query_time.as_deref().unwrap_or("-"),
            f.response_time.as_deref().unwrap_or("-"),
            f.latency_ms
                .map(|n| format!("{n:.3}"))
                .unwrap_or_else(|| "-".into()),
        )?;
    }

    writeln!(out, "  extra={extra}")?;
    Ok(())
}

fn write_rr_section(out: &mut impl Write, label: &str, rrs: &[String]) -> Result<()> {
    if rrs.is_empty() {
        return Ok(());
    }
    writeln!(out, "{label}:")?;
    for rr in rrs {
        writeln!(out, "    {rr}")?;
    }
    Ok(())
}

fn write_json(out: &mut impl Write, f: &DecodedFrame) -> Result<()> {
    serde_json::to_writer(&mut *out, f)?;
    writeln!(out)?;
    Ok(())
}

fn write_yaml(out: &mut impl Write, f: &DecodedFrame) -> Result<()> {
    serde_yaml::to_writer(&mut *out, f)?;
    writeln!(out)?;
    writeln!(out, "---")?;
    Ok(())
}
