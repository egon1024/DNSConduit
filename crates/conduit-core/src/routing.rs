//! Weighted pool/backend selection (spec pool-routing).

use conduit_config::effective_backend_weight;
use conduit_proto::config::{Backend, Config, Pool};
use std::net::SocketAddr;

#[derive(Debug, Clone)]
pub struct AttemptRecord {
    pub pool: String,
    pub backend: SocketAddr,
    pub attempt: u32,
}

/// Deterministic weighted pick: stable for tests given txn_id and snapshot generation.
pub fn select_backend(
    pools: &[Pool],
    pool_name: &str,
    txn_id: u64,
    snapshot_generation: u64,
) -> Option<(String, SocketAddr)> {
    let pool = pools.iter().find(|p| p.name == pool_name)?;
    if pool.backends.is_empty() {
        return None;
    }
    let total: u64 = pool
        .backends
        .iter()
        .map(|b| effective_backend_weight(b) as u64)
        .sum();
    let pick = (txn_id.wrapping_add(snapshot_generation)) % total;
    let mut acc = 0u64;
    for backend in &pool.backends {
        acc += effective_backend_weight(backend) as u64;
        if pick < acc {
            return parse_backend(pool_name, backend);
        }
    }
    parse_backend(pool_name, pool.backends.last()?)
}

fn parse_backend(pool_name: &str, backend: &Backend) -> Option<(String, SocketAddr)> {
    let addr: SocketAddr = backend.address.parse().ok()?;
    Some((pool_name.to_string(), addr))
}

pub fn default_pool_name(cfg: &Config) -> Option<String> {
    if let Some(pool) = cfg.pools.iter().find(|p| p.name == "default") {
        return Some(pool.name.clone());
    }
    cfg.pools.first().map(|p| p.name.clone())
}
