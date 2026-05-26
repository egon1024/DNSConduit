//! Compiled upstream forward settings (phase 1b slice A).

use conduit_proto::config::Config;
use std::collections::HashMap;
use std::net::{Ipv4Addr, SocketAddr};

pub const MAX_SOURCES_V4: usize = 32;
pub const DEFAULT_SOURCE_SELECTION: &str = "round_robin";

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

#[derive(Debug, Clone)]
pub struct CompiledForward {
    pub sources_v4: Vec<Ipv4Addr>,
    pub source_selection: String,
    pub timeout_ms: u32,
    pub outstanding_per_backend: u32,
}

#[derive(Debug, Clone, Default)]
pub struct CompiledPoolForward {
    pub sources_v4: Option<Vec<Ipv4Addr>>,
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
        let source_selection = if forward.source_selection.is_empty() {
            DEFAULT_SOURCE_SELECTION.to_string()
        } else {
            forward.source_selection.clone()
        };
        if source_selection != "round_robin" {
            return Err(format!(
                "forward.source_selection '{}' must be round_robin (slice A)",
                source_selection
            ));
        }

        let mut pool_forward = HashMap::new();
        for pool in &cfg.pools {
            if !pool.sources_v4.is_empty() {
                let sources = parse_sources_v4(&pool.sources_v4)?;
                pool_forward.insert(
                    pool.name.clone(),
                    CompiledPoolForward {
                        sources_v4: Some(sources),
                    },
                );
            }
        }

        Ok((
            CompiledForward {
                sources_v4,
                source_selection,
                timeout_ms: forward.timeout_ms,
                outstanding_per_backend: forward.outstanding_per_backend,
            },
            pool_forward,
        ))
    }
}

/// Until phase 1b slice B (IPv6 egress + `sources_v6`), reject IPv6 pool backends at config load.
/// **Slice B must remove or replace this** with family-consistent validation (v6 backend requires v6 sources).
pub fn validate_upstream_backend_addresses(cfg: &Config) -> Vec<String> {
    let mut errors = Vec::new();
    for pool in &cfg.pools {
        for backend in &pool.backends {
            let Ok(addr) = backend.address.parse::<SocketAddr>() else {
                continue;
            };
            if addr.is_ipv6() {
                errors.push(format!(
                    "pool '{}' backend '{}': IPv6 upstream addresses are not supported until phase 1b slice B (sources_v6 and IPv6 egress); use an IPv4 address",
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
