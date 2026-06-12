//! Weighted pool/backend selection (spec pool-routing).

use conduit_config::effective_backend_weight;
use conduit_proto::config::{Backend, Config, Pool};

/// Effective weight for pool selection (phase 5 hook for hot overrides in 5b).
#[inline]
pub fn resolve_backend_weight(backend: &Backend) -> u32 {
    effective_backend_weight(backend)
}
use std::net::SocketAddr;

#[derive(Debug, Clone)]
pub struct AttemptRecord {
    pub pool: String,
    pub backend: SocketAddr,
    pub attempt: u32,
}

/// Backends already used for `pool_name` on this transaction.
pub fn tried_backends_in_pool(attempts: &[AttemptRecord], pool_name: &str) -> Vec<SocketAddr> {
    attempts
        .iter()
        .filter(|a| a.pool == pool_name)
        .map(|a| a.backend)
        .collect()
}

/// Deterministic weighted pick: stable for tests given txn_id and snapshot generation.
///
/// On the first attempt (`attempt_count == 0`), all backends in the pool are candidates.
/// On retries (`attempt_count > 0`), backends already tried in this pool are excluded.
/// When every backend in the pool was already tried, returns `None` (pool exhausted).
pub fn select_backend(
    pools: &[Pool],
    pool_name: &str,
    txn_id: u64,
    snapshot_generation: u64,
    attempt_count: u32,
    tried_backends: &[SocketAddr],
) -> Option<(String, SocketAddr)> {
    let pool = pools.iter().find(|p| p.name == pool_name)?;
    if pool.backends.is_empty() {
        return None;
    }

    let candidates: Vec<&Backend> = pool
        .backends
        .iter()
        .filter(|backend| {
            let Ok(addr) = backend.address.parse::<SocketAddr>() else {
                return false;
            };
            if attempt_count > 0 && tried_backends.contains(&addr) {
                return false;
            }
            true
        })
        .collect();

    if candidates.is_empty() {
        return None;
    }

    let total: u64 = candidates
        .iter()
        .map(|b| resolve_backend_weight(b) as u64)
        .sum();
    if total == 0 {
        return None;
    }

    let pick = (txn_id.wrapping_add(snapshot_generation)) % total;
    let mut acc = 0u64;
    for backend in &candidates {
        acc += resolve_backend_weight(backend) as u64;
        if pick < acc {
            return parse_backend(pool_name, backend);
        }
    }
    parse_backend(pool_name, candidates.last()?)
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

#[cfg(test)]
mod tests {
    use super::*;
    use conduit_proto::config::Pool;

    fn pool_three_backends() -> Pool {
        Pool {
            name: "primary".into(),
            backends: vec![
                Backend {
                    address: "10.0.0.1:53".into(),
                    weight: Some(100),
                },
                Backend {
                    address: "10.0.0.2:53".into(),
                    weight: Some(100),
                },
                Backend {
                    address: "10.0.0.3:53".into(),
                    weight: Some(100),
                },
            ],
            sources_v4: vec![],
            sources_v6: vec![],
        }
    }

    #[test]
    fn first_attempt_ignores_tried_list() {
        let pools = vec![pool_three_backends()];
        let tried = vec!["10.0.0.1:53".parse().unwrap()];
        let (_, addr) = select_backend(&pools, "primary", 5, 0, 0, &tried).unwrap();
        assert_eq!(addr, "10.0.0.1:53".parse().unwrap());
    }

    #[test]
    fn retry_excludes_tried_backend_in_pool() {
        let pools = vec![pool_three_backends()];
        let b1 = "10.0.0.1:53".parse().unwrap();
        let (_, first) = select_backend(&pools, "primary", 2, 0, 0, &[]).unwrap();
        let (_, second) = select_backend(&pools, "primary", 2, 0, 1, &[first]).unwrap();
        assert_ne!(first, second);
        assert_eq!(first, b1);
        let (_, third) = select_backend(&pools, "primary", 2, 0, 2, &[first, second]).unwrap();
        assert_ne!(third, first);
        assert_ne!(third, second);
    }

    #[test]
    fn retry_returns_none_when_pool_exhausted() {
        let pools = vec![pool_three_backends()];
        let b1 = "10.0.0.1:53".parse().unwrap();
        let b2 = "10.0.0.2:53".parse().unwrap();
        let b3 = "10.0.0.3:53".parse().unwrap();
        assert!(select_backend(&pools, "primary", 2, 0, 3, &[b1, b2, b3]).is_none());
    }

    #[test]
    fn cross_pool_retry_does_not_exclude_other_pools() {
        let pools = vec![
            pool_three_backends(),
            Pool {
                name: "secondary".into(),
                backends: vec![Backend {
                    address: "10.0.1.1:53".into(),
                    weight: Some(100),
                }],
                sources_v4: vec![],
                sources_v6: vec![],
            },
        ];
        let primary_tried = vec!["10.0.0.1:53".parse().unwrap()];
        let (_, addr) = select_backend(&pools, "secondary", 7, 0, 1, &primary_tried).unwrap();
        assert_eq!(addr, "10.0.1.1:53".parse().unwrap());
    }
}
