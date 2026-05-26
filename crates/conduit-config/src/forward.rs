//! Compiled upstream forward settings (phase 1b).

use conduit_proto::config::Config;
use std::collections::HashMap;
use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr};

pub const MAX_SOURCES_V4: usize = 32;
pub const MAX_SOURCES_V6: usize = 32;
pub const DEFAULT_SOURCE_SELECTION: &str = "round_robin";
pub const DEFAULT_UPSTREAM_TRANSPORT: &str = "udp_only";

/// Internal wire policy for upstream RD bit (Rhai `set_rd` / `clear_rd` or preserve).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecursionDesired {
    Preserve,
    Clear,
    Set,
}

impl RecursionDesired {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Preserve => "preserve",
            Self::Clear => "clear",
            Self::Set => "set",
        }
    }
}

/// Upstream transport policy (phase 1b slice C).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum UpstreamTransport {
    #[default]
    UdpOnly,
    TcpOnly,
    PreferUdpWithTcpFallback,
}

impl UpstreamTransport {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::UdpOnly => "udp_only",
            Self::TcpOnly => "tcp_only",
            Self::PreferUdpWithTcpFallback => "prefer_udp_with_tcp_fallback",
        }
    }
}

pub fn parse_upstream_transport(raw: &str) -> Result<UpstreamTransport, String> {
    match raw.trim() {
        "" | "udp_only" => Ok(UpstreamTransport::UdpOnly),
        "tcp_only" => Ok(UpstreamTransport::TcpOnly),
        "prefer_udp_with_tcp_fallback" => Ok(UpstreamTransport::PreferUdpWithTcpFallback),
        other => Err(format!(
            "forward.upstream_transport '{other}' must be udp_only, tcp_only, or prefer_udp_with_tcp_fallback"
        )),
    }
}

#[derive(Debug, Clone)]
pub struct CompiledForward {
    pub sources_v4: Vec<Ipv4Addr>,
    pub sources_v6: Vec<Ipv6Addr>,
    pub source_selection: String,
    pub upstream_transport: UpstreamTransport,
    pub client_tcp_uses_upstream_tcp: bool,
    pub timeout_ms: u32,
    pub outstanding_per_backend: u32,
}

#[derive(Debug, Clone, Default)]
pub struct CompiledPoolForward {
    pub sources_v4: Option<Vec<Ipv4Addr>>,
    pub sources_v6: Option<Vec<Ipv6Addr>>,
}

pub fn compile_forward_from_config(
    cfg: &Config,
) -> Result<(CompiledForward, HashMap<String, CompiledPoolForward>), String> {
    CompiledForward::compile_from_config(cfg)
}

impl CompiledForward {
    pub fn compile_from_config(
        cfg: &Config,
    ) -> Result<(Self, HashMap<String, CompiledPoolForward>), String> {
        let backend_errors = validate_upstream_backend_addresses(cfg);
        if !backend_errors.is_empty() {
            return Err(backend_errors.join("; "));
        }

        let forward = cfg
            .forward
            .as_ref()
            .ok_or_else(|| "forward section required".to_string())?;
        let sources_v4 = parse_sources_v4(&forward.sources_v4)?;
        let sources_v6 = parse_sources_v6(&forward.sources_v6)?;
        let source_selection = if forward.source_selection.is_empty() {
            DEFAULT_SOURCE_SELECTION.to_string()
        } else {
            forward.source_selection.clone()
        };
        if source_selection != "round_robin" {
            return Err(format!(
                "forward.source_selection '{}' must be round_robin",
                source_selection
            ));
        }
        let upstream_transport = parse_upstream_transport(&forward.upstream_transport)?;

        let mut pool_forward = HashMap::new();
        for pool in &cfg.pools {
            let sources_v4 = if !pool.sources_v4.is_empty() {
                Some(parse_sources_v4(&pool.sources_v4)?)
            } else {
                None
            };
            let sources_v6 = if !pool.sources_v6.is_empty() {
                Some(parse_sources_v6(&pool.sources_v6)?)
            } else {
                None
            };
            if sources_v4.is_some() || sources_v6.is_some() {
                pool_forward.insert(
                    pool.name.clone(),
                    CompiledPoolForward {
                        sources_v4,
                        sources_v6,
                    },
                );
            }
        }

        Ok((
            CompiledForward {
                sources_v4,
                sources_v6,
                source_selection,
                upstream_transport,
                client_tcp_uses_upstream_tcp: forward.client_tcp_uses_upstream_tcp,
                timeout_ms: forward.timeout_ms,
                outstanding_per_backend: forward.outstanding_per_backend,
            },
            pool_forward,
        ))
    }
}

/// Validate pool backend addresses parse as socket addresses.
pub fn validate_upstream_backend_addresses(cfg: &Config) -> Vec<String> {
    let mut errors = Vec::new();
    for pool in &cfg.pools {
        for backend in &pool.backends {
            if backend.address.parse::<SocketAddr>().is_err() {
                errors.push(format!(
                    "pool '{}' backend '{}': invalid socket address",
                    pool.name, backend.address
                ));
            }
        }
    }
    errors
}

pub fn parse_sources_v4(raw: &[String]) -> Result<Vec<Ipv4Addr>, String> {
    if raw.len() > MAX_SOURCES_V4 {
        return Err(format!(
            "sources_v4 has {} entries; maximum is {MAX_SOURCES_V4}",
            raw.len()
        ));
    }
    let mut out = Vec::with_capacity(raw.len());
    for (i, s) in raw.iter().enumerate() {
        if s.trim().is_empty() {
            return Err(format!("sources_v4[{i}] must not be empty"));
        }
        let addr: Ipv4Addr = s
            .parse()
            .map_err(|_| format!("sources_v4[{i}] '{s}' is not a valid IPv4 address"))?;
        out.push(addr);
    }
    Ok(out)
}

pub fn parse_sources_v6(raw: &[String]) -> Result<Vec<Ipv6Addr>, String> {
    if raw.len() > MAX_SOURCES_V6 {
        return Err(format!(
            "sources_v6 has {} entries; maximum is {MAX_SOURCES_V6}",
            raw.len()
        ));
    }
    let mut out = Vec::with_capacity(raw.len());
    for (i, s) in raw.iter().enumerate() {
        if s.trim().is_empty() {
            return Err(format!("sources_v6[{i}] must not be empty"));
        }
        let addr: Ipv6Addr = s
            .parse()
            .map_err(|_| format!("sources_v6[{i}] '{s}' is not a valid IPv6 address"))?;
        out.push(addr);
    }
    Ok(out)
}
