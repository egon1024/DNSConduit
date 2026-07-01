//! Compiled backend health-probe configuration (phase 1c).
//!
//! This module owns the *probe configuration* contract: how the YAML `health`
//! block on a pool (plus per-backend overrides) maps to a normalized,
//! validated [`CompiledHealth`]. Runtime probe *state* (observed/applied
//! health, rise/fall counters, latency EWMA) lives outside the snapshot in
//! `conduit-core` (design §D9); nothing mutable lives here.

use conduit_dns_wire::{Rcode, RecordType};
use conduit_proto::config::{Backend, Config, HealthCheck, Pool};
use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};

/// Default probe period when `interval_ms` is unset (design §D5).
pub const DEFAULT_HEALTH_INTERVAL_MS: u32 = 1000;
/// Minimum allowed probe interval (design §D5).
pub const HEALTH_INTERVAL_FLOOR_MS: u32 = 100;
/// Default consecutive successes to mark a backend up (design §D3).
pub const DEFAULT_HEALTH_RISE: u32 = 3;
/// Default consecutive failures to mark a backend down (design §D3).
pub const DEFAULT_HEALTH_FALL: u32 = 2;
/// Default consecutive passive (live-traffic) failures to open (design §D1).
pub const DEFAULT_HEALTH_PASSIVE_FALL: u32 = 2;
/// Default EWMA smoothing factor for the latency estimate (design §D3).
pub const DEFAULT_LATENCY_EWMA_ALPHA: f64 = 0.2;
/// Default floor for the latency weight factor — latency never zeroes a
/// backend, only liveness does (design §D3).
pub const DEFAULT_LATENCY_FLOOR: f64 = 0.25;
/// Default probe query name when no pool template is configured.
pub const DEFAULT_PROBE_QNAME: &str = ".";
/// Default probe query type when no pool template is configured.
pub const DEFAULT_PROBE_QTYPE: &str = "NS";

/// Initial `applied_health` policy for a backend (design §D10).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum InitialHealthState {
    /// Eligible immediately until probes prove otherwise (default).
    #[default]
    Optimistic,
    /// Eligible after one successful probe.
    Require1Good,
    /// Eligible after the full rise count.
    RequireFullRise,
}

impl InitialHealthState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Optimistic => "optimistic",
            Self::Require1Good => "require_1_good",
            Self::RequireFullRise => "require_full_rise",
        }
    }

    pub fn parse(raw: &str) -> Result<Self, String> {
        match raw.trim() {
            "" | "optimistic" => Ok(Self::Optimistic),
            "require_1_good" => Ok(Self::Require1Good),
            "require_full_rise" => Ok(Self::RequireFullRise),
            other => Err(format!(
                "health.initial_state '{other}' must be optimistic, require_1_good, or require_full_rise"
            )),
        }
    }
}

/// Compiled per-backend probe semantics (pool template merged with overrides).
#[derive(Debug, Clone, PartialEq)]
pub struct CompiledBackendHealth {
    pub address: SocketAddr,
    /// Configured backend `name` when set (operator selectors and metric labels).
    pub name: Option<String>,
    /// Metric/log label: backend `name` when set, else the address string.
    pub label: String,
    pub probe_qname: String,
    pub probe_qtype: u16,
    pub probe_source: Option<IpAddr>,
}

/// Compiled per-pool probe configuration (only for pools with health enabled).
#[derive(Debug, Clone, PartialEq)]
pub struct CompiledPoolHealth {
    pub interval_ms: u32,
    pub timeout_ms: u32,
    pub rise: u32,
    pub fall: u32,
    /// `None` = accept any well-formed response (design §D4); `Some` = narrowed
    /// set of acceptable rcode numbers.
    pub acceptable_rcodes: Option<Vec<u16>>,
    pub initial_state: InitialHealthState,
    pub latency_weighting: bool,
    pub latency_ewma_alpha: f64,
    pub latency_floor: f64,
    pub min_eligible: u32,
    pub passive_fast_trip: bool,
    pub passive_fall: u32,
    pub backends: Vec<CompiledBackendHealth>,
}

/// Compiled health configuration for the whole snapshot. Pools without an
/// enabled `health` block are absent (probing disabled, today's behavior).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CompiledHealth {
    pub pools: HashMap<String, CompiledPoolHealth>,
}

impl CompiledHealth {
    pub fn is_empty(&self) -> bool {
        self.pools.is_empty()
    }

    /// True when active probing is enabled for the named pool.
    pub fn enabled_for(&self, pool: &str) -> bool {
        self.pools.contains_key(pool)
    }

    pub fn pool(&self, pool: &str) -> Option<&CompiledPoolHealth> {
        self.pools.get(pool)
    }

    /// Resolve a backend selector within a pool: `host:port` or configured `name`.
    pub fn resolve_backend(&self, pool: &str, identifier: &str) -> Result<SocketAddr, String> {
        let Some(pool_cfg) = self.pool(pool) else {
            return Err(format!("unknown pool '{pool}'"));
        };
        pool_cfg.resolve_backend(identifier)
    }
}

impl CompiledPoolHealth {
    /// Resolve `host:port` or a configured backend `name` to a socket address.
    pub fn resolve_backend(&self, identifier: &str) -> Result<SocketAddr, String> {
        if let Ok(addr) = identifier.parse::<SocketAddr>() {
            if self.backends.iter().any(|b| b.address == addr) {
                return Ok(addr);
            }
            return Err(format!("backend '{addr}' not in pool"));
        }
        for backend in &self.backends {
            if backend.name.as_deref() == Some(identifier) {
                return Ok(backend.address);
            }
        }
        Err(format!("unknown backend name '{identifier}'"))
    }
}

/// Backend metric/log label: configured `name` when set, else `address`.
/// Mirrors `conduit_core::routing::backend_metric_label` (kept inline here to
/// avoid a config → core dependency cycle).
fn backend_label(backend: &Backend) -> String {
    backend
        .name
        .as_ref()
        .filter(|n| !n.is_empty())
        .cloned()
        .unwrap_or_else(|| backend.address.clone())
}

fn parse_qtype(raw: &str) -> Result<u16, String> {
    RecordType::parse_name_or_err(raw).map(RecordType::number)
}

fn parse_rcode(raw: &str) -> Result<u16, String> {
    Rcode::parse_name_or_err(raw).map(Rcode::number)
}

fn nonempty(opt: Option<&String>) -> Option<&str> {
    opt.map(|s| s.as_str()).filter(|s| !s.trim().is_empty())
}

/// Compile the health configuration from a validated `Config`.
///
/// Returns an error string on the first malformed value (bad rcode/qtype name,
/// unparseable probe source, etc.); callers building a snapshot treat this as a
/// hard failure, mirroring `CompiledForward::compile_from_config`.
pub fn compile_health_from_config(cfg: &Config) -> Result<CompiledHealth, String> {
    let mut pools = HashMap::new();
    for pool in &cfg.pools {
        let Some(hc) = pool.health.as_ref() else {
            continue;
        };
        if !hc.enabled {
            continue;
        }
        let compiled = compile_pool_health(pool, hc)?;
        pools.insert(pool.name.clone(), compiled);
    }
    Ok(CompiledHealth { pools })
}

fn compile_pool_health(pool: &Pool, hc: &HealthCheck) -> Result<CompiledPoolHealth, String> {
    let interval_ms = hc.interval_ms.unwrap_or(DEFAULT_HEALTH_INTERVAL_MS);
    let timeout_ms = hc.timeout_ms.unwrap_or(interval_ms);
    let rise = hc.rise.unwrap_or(DEFAULT_HEALTH_RISE);
    let fall = hc.fall.unwrap_or(DEFAULT_HEALTH_FALL);
    let passive_fall = hc.passive_fall.unwrap_or(DEFAULT_HEALTH_PASSIVE_FALL);

    let pool_qname = nonempty(hc.probe_qname.as_ref())
        .unwrap_or(DEFAULT_PROBE_QNAME)
        .to_string();
    let pool_qtype_raw = nonempty(hc.probe_qtype.as_ref()).unwrap_or(DEFAULT_PROBE_QTYPE);
    let pool_qtype = parse_qtype(pool_qtype_raw)
        .map_err(|e| format!("pool '{}' health.probe_qtype: {e}", pool.name))?;

    let acceptable_rcodes = if hc.acceptable_rcodes.is_empty() {
        None
    } else {
        let mut out = Vec::with_capacity(hc.acceptable_rcodes.len());
        for name in &hc.acceptable_rcodes {
            let code = parse_rcode(name)
                .map_err(|e| format!("pool '{}' health.acceptable_rcodes: {e}", pool.name))?;
            out.push(code);
        }
        Some(out)
    };

    let initial_state = InitialHealthState::parse(hc.initial_state.as_deref().unwrap_or(""))
        .map_err(|e| format!("pool '{}' {e}", pool.name))?;

    let mut backends = Vec::with_capacity(pool.backends.len());
    for backend in &pool.backends {
        let address: SocketAddr = backend.address.parse().map_err(|_| {
            format!(
                "pool '{}' backend '{}': invalid socket address",
                pool.name, backend.address
            )
        })?;
        let probe_qname = nonempty(backend.probe_qname.as_ref())
            .unwrap_or(&pool_qname)
            .to_string();
        let probe_qtype = match nonempty(backend.probe_qtype.as_ref()) {
            Some(raw) => parse_qtype(raw).map_err(|e| {
                format!(
                    "pool '{}' backend '{}' probe_qtype: {e}",
                    pool.name, backend.address
                )
            })?,
            None => pool_qtype,
        };
        let probe_source = match nonempty(backend.probe_source.as_ref()) {
            Some(raw) => Some(raw.parse::<IpAddr>().map_err(|_| {
                format!(
                    "pool '{}' backend '{}' probe_source '{}' is not a valid IP address",
                    pool.name, backend.address, raw
                )
            })?),
            None => None,
        };
        backends.push(CompiledBackendHealth {
            address,
            name: backend.name.as_ref().filter(|n| !n.is_empty()).cloned(),
            label: backend_label(backend),
            probe_qname,
            probe_qtype,
            probe_source,
        });
    }

    Ok(CompiledPoolHealth {
        interval_ms,
        timeout_ms,
        rise,
        fall,
        acceptable_rcodes,
        initial_state,
        latency_weighting: hc.latency_weighting.unwrap_or(false),
        latency_ewma_alpha: DEFAULT_LATENCY_EWMA_ALPHA,
        latency_floor: DEFAULT_LATENCY_FLOOR,
        min_eligible: hc.min_eligible.unwrap_or(0),
        passive_fast_trip: hc.passive_fast_trip.unwrap_or(true),
        passive_fall,
        backends,
    })
}

/// Validate the health blocks of `cfg`, collecting human-readable errors.
pub fn validate_health(cfg: &Config) -> Vec<String> {
    let mut errors = Vec::new();
    for pool in &cfg.pools {
        let Some(hc) = pool.health.as_ref() else {
            continue;
        };
        // Validate the surface even when disabled so a typo is caught before
        // an operator flips `enabled: true`.
        if let Some(interval) = hc.interval_ms {
            if interval < HEALTH_INTERVAL_FLOOR_MS {
                errors.push(format!(
                    "pool '{}' health.interval_ms {interval} is below the {HEALTH_INTERVAL_FLOOR_MS}ms floor",
                    pool.name
                ));
            }
        }
        if hc.timeout_ms == Some(0) {
            errors.push(format!(
                "pool '{}' health.timeout_ms must be >= 1 when set",
                pool.name
            ));
        }
        if hc.rise == Some(0) {
            errors.push(format!("pool '{}' health.rise must be >= 1", pool.name));
        }
        if hc.fall == Some(0) {
            errors.push(format!("pool '{}' health.fall must be >= 1", pool.name));
        }
        if hc.passive_fall == Some(0) {
            errors.push(format!(
                "pool '{}' health.passive_fall must be >= 1 when set",
                pool.name
            ));
        }
        if let Some(raw) = nonempty(hc.probe_qtype.as_ref()) {
            if let Err(e) = parse_qtype(raw) {
                errors.push(format!("pool '{}' health.probe_qtype: {e}", pool.name));
            }
        }
        for name in &hc.acceptable_rcodes {
            if let Err(e) = parse_rcode(name) {
                errors.push(format!(
                    "pool '{}' health.acceptable_rcodes: {e}",
                    pool.name
                ));
            }
        }
        if let Some(raw) = hc.initial_state.as_deref() {
            if let Err(e) = InitialHealthState::parse(raw) {
                errors.push(format!("pool '{}' {e}", pool.name));
            }
        }
        for backend in &pool.backends {
            if let Some(raw) = nonempty(backend.probe_qtype.as_ref()) {
                if let Err(e) = parse_qtype(raw) {
                    errors.push(format!(
                        "pool '{}' backend '{}' probe_qtype: {e}",
                        pool.name, backend.address
                    ));
                }
            }
            if let Some(raw) = nonempty(backend.probe_source.as_ref()) {
                if raw.parse::<IpAddr>().is_err() {
                    errors.push(format!(
                        "pool '{}' backend '{}' probe_source '{}' is not a valid IP address",
                        pool.name, backend.address, raw
                    ));
                }
            }
            if let Some(raw) = nonempty(backend.transport.as_ref()) {
                if let Err(e) = crate::forward::parse_upstream_transport(raw) {
                    errors.push(format!(
                        "pool '{}' backend '{}' transport: {e}",
                        pool.name, backend.address
                    ));
                }
            }
        }
    }
    errors
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::file::load_yaml;
    use crate::validate::validate;

    fn compile(fixture: &str) -> CompiledHealth {
        let cfg = load_yaml(fixture).unwrap();
        compile_health_from_config(&cfg).unwrap()
    }

    #[test]
    fn no_health_block_compiles_empty() {
        let health = compile(include_str!("../../../tests/fixtures/config/minimal.yaml"));
        assert!(health.is_empty());
        assert!(!health.enabled_for("default"));
    }

    #[test]
    fn disabled_health_not_compiled() {
        let cfg = load_yaml(include_str!(
            "../../../tests/fixtures/config/health-disabled.yaml"
        ))
        .unwrap();
        assert!(cfg.pools[0].health.is_some());
        let health = compile_health_from_config(&cfg).unwrap();
        assert!(health.is_empty(), "disabled pool must not probe");
    }

    #[test]
    fn enabled_health_applies_defaults() {
        let health = compile(include_str!(
            "../../../tests/fixtures/config/with-health-backend-override.yaml"
        ));
        let pool = health.pool("default").expect("default pool health");
        // interval explicitly 1000; rise/fall/passive defaults; timeout defaults to interval.
        assert_eq!(pool.interval_ms, DEFAULT_HEALTH_INTERVAL_MS);
        assert_eq!(pool.timeout_ms, DEFAULT_HEALTH_INTERVAL_MS);
        assert_eq!(pool.rise, DEFAULT_HEALTH_RISE);
        assert_eq!(pool.fall, DEFAULT_HEALTH_FALL);
        assert_eq!(pool.passive_fall, DEFAULT_HEALTH_PASSIVE_FALL);
        assert!(pool.passive_fast_trip, "passive defaults on (design D11)");
        assert!((pool.latency_ewma_alpha - DEFAULT_LATENCY_EWMA_ALPHA).abs() < f64::EPSILON);
        assert!((pool.latency_floor - DEFAULT_LATENCY_FLOOR).abs() < f64::EPSILON);
        assert_eq!(pool.initial_state, InitialHealthState::Optimistic);
        assert!(
            pool.acceptable_rcodes.is_none(),
            "default accepts any rcode"
        );
    }

    #[test]
    fn merges_pool_template_with_backend_override() {
        let health = compile(include_str!(
            "../../../tests/fixtures/config/with-health-backend-override.yaml"
        ));
        let pool = health.pool("default").unwrap();
        let primary = pool.backends.iter().find(|b| b.label == "primary").unwrap();
        let secondary = pool
            .backends
            .iter()
            .find(|b| b.label == "secondary")
            .unwrap();
        // Primary inherits the pool template.
        assert_eq!(primary.probe_qname, "pool-default.example.");
        assert_eq!(primary.probe_qtype, 1); // A
        assert!(primary.probe_source.is_none());
        // Secondary overrides qname/qtype/source.
        assert_eq!(secondary.probe_qname, "secondary.example.");
        assert_eq!(secondary.probe_qtype, 6); // SOA
        assert_eq!(
            secondary.probe_source,
            Some("127.0.0.1".parse::<std::net::IpAddr>().unwrap())
        );
    }

    #[test]
    fn narrowed_acceptable_rcodes_resolve_to_numbers() {
        let health = compile(include_str!(
            "../../../tests/fixtures/config/with-health.yaml"
        ));
        let pool = health.pool("default").unwrap();
        assert_eq!(pool.acceptable_rcodes.as_deref(), Some(&[0u16, 3u16][..])); // NOERROR, NXDOMAIN
        assert!(pool.latency_weighting);
        assert_eq!(pool.min_eligible, 1);
    }

    #[test]
    fn with_health_round_trips_through_export() {
        // Default backend weight (100) is intentionally omitted on export, so
        // compare the health block (and recompiled probe config) rather than
        // the full pool, which differs only in `weight: Some(100)` vs `None`.
        let cfg = load_yaml(include_str!(
            "../../../tests/fixtures/config/with-health.yaml"
        ))
        .unwrap();
        let exported = crate::export_yaml(&cfg).unwrap();
        assert!(exported.contains("health:"));
        let reparsed = load_yaml(&exported).unwrap();
        assert_eq!(reparsed.pools[0].health, cfg.pools[0].health);
        assert_eq!(
            compile_health_from_config(&reparsed).unwrap(),
            compile_health_from_config(&cfg).unwrap()
        );
    }

    #[test]
    fn backend_override_round_trips_through_export() {
        let cfg = load_yaml(include_str!(
            "../../../tests/fixtures/config/with-health-backend-override.yaml"
        ))
        .unwrap();
        let exported = crate::export_yaml(&cfg).unwrap();
        let reparsed = load_yaml(&exported).unwrap();
        assert_eq!(reparsed.pools[0].health, cfg.pools[0].health);
        for (a, b) in reparsed.pools[0]
            .backends
            .iter()
            .zip(&cfg.pools[0].backends)
        {
            assert_eq!(a.probe_qname, b.probe_qname);
            assert_eq!(a.probe_qtype, b.probe_qtype);
            assert_eq!(a.probe_source, b.probe_source);
            assert_eq!(a.transport, b.transport);
        }
        assert_eq!(
            compile_health_from_config(&reparsed).unwrap(),
            compile_health_from_config(&cfg).unwrap()
        );
    }

    #[test]
    fn validate_accepts_with_health() {
        let cfg = load_yaml(include_str!(
            "../../../tests/fixtures/config/with-health.yaml"
        ))
        .unwrap();
        let result = validate(&cfg);
        assert!(result.ok, "errors: {:?}", result.errors);
    }

    #[test]
    fn validate_rejects_interval_below_floor_and_bad_rcode() {
        let cfg = load_yaml(include_str!(
            "../../../tests/fixtures/config/health-invalid.yaml"
        ))
        .unwrap();
        let result = validate(&cfg);
        assert!(!result.ok);
        assert!(
            result.errors.iter().any(|e| e.contains("floor")),
            "expected interval-floor error: {:?}",
            result.errors
        );
        assert!(
            result
                .errors
                .iter()
                .any(|e| e.contains("acceptable_rcodes")),
            "expected bad-rcode error: {:?}",
            result.errors
        );
    }

    #[test]
    fn validate_rejects_zero_rise() {
        let mut cfg = load_yaml(include_str!(
            "../../../tests/fixtures/config/with-health.yaml"
        ))
        .unwrap();
        cfg.pools[0].health.as_mut().unwrap().rise = Some(0);
        let result = validate(&cfg);
        assert!(!result.ok);
        assert!(result.errors.iter().any(|e| e.contains("rise")));
    }

    #[test]
    fn validate_rejects_bad_initial_state() {
        let mut cfg = load_yaml(include_str!(
            "../../../tests/fixtures/config/with-health.yaml"
        ))
        .unwrap();
        cfg.pools[0].health.as_mut().unwrap().initial_state = Some("sometimes".into());
        let result = validate(&cfg);
        assert!(!result.ok);
        assert!(result.errors.iter().any(|e| e.contains("initial_state")));
    }

    #[test]
    fn validate_rejects_divergent_transport_name() {
        let mut cfg = load_yaml(include_str!(
            "../../../tests/fixtures/config/with-health.yaml"
        ))
        .unwrap();
        cfg.pools[0].backends[0].transport = Some("carrier_pigeon".into());
        let result = validate(&cfg);
        assert!(!result.ok);
        assert!(result.errors.iter().any(|e| e.contains("transport")));
    }

    #[test]
    fn resolve_backend_by_address_or_name() {
        let health = compile(include_str!(
            "../../../tests/fixtures/config/with-health-backend-override.yaml"
        ));
        let addr = "127.0.0.1:5300".parse().unwrap();
        assert_eq!(
            health.resolve_backend("default", "127.0.0.1:5300").unwrap(),
            addr
        );
        assert_eq!(
            health.resolve_backend("default", "primary").unwrap(),
            addr
        );
        assert!(health.resolve_backend("default", "missing").is_err());
    }
}
