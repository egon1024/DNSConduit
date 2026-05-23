//! Compile observation config for runtime snapshots.

use crate::queue::DropPolicy;
use conduit_proto::config::{Config, ObservationConfig, ObservationSink};
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct CompiledObservation {
    pub enabled: bool,
    pub queue_depth: usize,
    pub drop_policy: DropPolicy,
    pub sinks: Vec<CompiledSinkInstance>,
}

#[derive(Debug, Clone)]
pub struct CompiledSinkInstance {
    pub emit_query: bool,
    pub emit_response: bool,
    pub emit_retry: bool,
    pub tag_required: Option<String>,
    pub export_id: String,
    pub destinations: Vec<Destination>,
}

#[derive(Debug, Clone)]
pub enum Destination {
    Unix(PathBuf),
    Tcp { host: String, port: u16 },
}

pub fn compile_from_config(cfg: &Config) -> CompiledObservation {
    let obs = cfg.observation.as_ref();
    let (queue_depth, drop_policy, sinks) = match obs {
        Some(o) => (
            default_queue_depth(o),
            parse_drop_policy(o),
            compile_sinks(o),
        ),
        None => (8192, DropPolicy::DropOldest, Vec::new()),
    };
    let enabled = !sinks.is_empty();
    CompiledObservation {
        enabled,
        queue_depth,
        drop_policy,
        sinks,
    }
}

fn default_queue_depth(o: &ObservationConfig) -> usize {
    if o.queue_depth == 0 {
        8192
    } else {
        o.queue_depth as usize
    }
}

fn parse_drop_policy(o: &ObservationConfig) -> DropPolicy {
    DropPolicy::parse(o.drop_policy.as_str()).unwrap_or(DropPolicy::DropOldest)
}

fn compile_sinks(o: &ObservationConfig) -> Vec<CompiledSinkInstance> {
    o.sinks.iter().filter_map(compile_one_sink).collect()
}

pub(crate) fn compile_one_sink(s: &ObservationSink) -> Option<CompiledSinkInstance> {
    if s.r#type != "dnstap" || s.export_id.is_empty() || s.destinations.is_empty() {
        return None;
    }
    let destinations: Vec<Destination> = s
        .destinations
        .iter()
        .filter_map(|d| parse_destination(d.as_str()))
        .collect();
    if destinations.is_empty() {
        return None;
    }
    let emit = normalize_emit(&s.emit);
    let tag_required = s
        .filters
        .as_ref()
        .and_then(|f| f.tag_required.clone())
        .filter(|t| !t.is_empty());
    Some(CompiledSinkInstance {
        emit_query: emit.query,
        emit_response: emit.response,
        emit_retry: emit.retry,
        tag_required,
        export_id: s.export_id.clone(),
        destinations,
    })
}

struct EmitFlags {
    query: bool,
    response: bool,
    retry: bool,
}

fn normalize_emit(emit: &[String]) -> EmitFlags {
    let mut flags = EmitFlags {
        query: false,
        response: false,
        retry: false,
    };
    for e in emit {
        match e.as_str() {
            "query" => flags.query = true,
            "response" => flags.response = true,
            "retry" => flags.retry = true,
            _ => {}
        }
    }
    if !flags.query && !flags.response && !flags.retry {
        flags.query = true;
        flags.response = true;
    }
    flags
}

pub fn parse_destination(s: &str) -> Option<Destination> {
    if let Some(path) = s.strip_prefix("unix:") {
        return Some(Destination::Unix(PathBuf::from(path)));
    }
    if let Some(rest) = s.strip_prefix("tcp:") {
        let (host, port) = rest.rsplit_once(':')?;
        let port: u16 = port.parse().ok()?;
        if host.is_empty() {
            return None;
        }
        return Some(Destination::Tcp {
            host: host.to_string(),
            port,
        });
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_unix_and_tcp_destinations() {
        assert!(matches!(
            parse_destination("unix:/tmp/x.sock"),
            Some(Destination::Unix(_))
        ));
        assert!(matches!(
            parse_destination("tcp:127.0.0.1:6000"),
            Some(Destination::Tcp { .. })
        ));
        assert!(parse_destination("bad").is_none());
    }
}
