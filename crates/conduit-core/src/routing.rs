//! Weighted pool/backend selection (spec pool-routing).

use crate::health::{BackendKey, Health, HealthTable};
use conduit_config::effective_backend_weight;
use conduit_config::health::CompiledPoolHealth;
use conduit_proto::config::{Backend, Config, Listener, Pool};

/// Effective weight for pool selection (phase 5 hook for hot overrides in 5b).
#[inline]
pub fn resolve_backend_weight(backend: &Backend) -> u32 {
    effective_backend_weight(backend)
}
use std::net::SocketAddr;

/// Health inputs the Route phase feeds into selection for one pool (design
/// §D7). `None` at the call site means health is unconfigured for the pool, so
/// selection behaves exactly as before this feature (all backends eligible at
/// configured weight).
#[derive(Clone, Copy)]
pub struct PoolHealthView<'a> {
    /// Compiled probe config for the pool (latency weighting, fail-open floor).
    pub config: &'a CompiledPoolHealth,
    /// Lock-free snapshot of the runtime health side-table.
    pub table: &'a HealthTable,
}

/// A backend is eligible when its `applied_health` is up. A backend with no
/// side-table entry (should not happen once reconciled) is treated as eligible
/// so missing state never silently drops a backend.
fn backend_eligible(table: &HealthTable, pool: &str, addr: SocketAddr) -> bool {
    match table.get(&BackendKey::new(pool.to_string(), addr)) {
        Some(state) => state.applied() == Health::Up,
        None => true,
    }
}

/// Configured weight scaled by a latency factor, kept as an integer weight so
/// the deterministic modulo pick is unchanged when the factor is `1.0`. The
/// result is at least 1 so an eligible backend never drops out via rounding —
/// only liveness (eligibility) removes a backend, never latency (design §D3).
fn effective_weight(configured: u32, factor: f64) -> u64 {
    let scaled = (configured as f64 * factor).round() as i64;
    scaled.max(1) as u64
}

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
///
/// When `health` is `Some`, this is the **single** health-aware selection path
/// (design §D7): candidates are filtered to the eligible (`applied == up`)
/// subset, the **fail-open floor** restores all candidates when too few are
/// eligible (a single-backend pool always fails open), and — with latency
/// weighting on — each eligible backend's configured weight is scaled by its
/// damped latency factor before the same weighted pick runs. With `health` of
/// `None` the behavior is byte-for-byte the pre-health weighted pick.
pub fn select_backend(
    pools: &[Pool],
    pool_name: &str,
    txn_id: u64,
    snapshot_generation: u64,
    attempt_count: u32,
    tried_backends: &[SocketAddr],
    health: Option<PoolHealthView<'_>>,
) -> Option<(String, SocketAddr)> {
    let pool = pools.iter().find(|p| p.name == pool_name)?;
    if pool.backends.is_empty() {
        return None;
    }

    // Candidates: parseable backends not already tried on this transaction.
    let candidates: Vec<(&Backend, SocketAddr)> = pool
        .backends
        .iter()
        .filter_map(|backend| {
            let addr = backend.address.parse::<SocketAddr>().ok()?;
            if attempt_count > 0 && tried_backends.contains(&addr) {
                return None;
            }
            Some((backend, addr))
        })
        .collect();

    if candidates.is_empty() {
        return None;
    }

    // Eligibility filter + fail-open floor (design §D7). `use_latency` is only
    // set when health is active, not failing open, and latency weighting is on.
    let (chosen, use_latency): (Vec<(&Backend, SocketAddr)>, bool) = match health {
        Some(view) => {
            let eligible: Vec<(&Backend, SocketAddr)> = candidates
                .iter()
                .copied()
                .filter(|(_, addr)| backend_eligible(view.table, pool_name, *addr))
                .collect();
            // The floor is at least 1 (Route always needs an eligible backend);
            // a single-backend pool always fails open.
            let floor = (view.config.min_eligible.max(1)) as usize;
            let fail_open = pool.backends.len() <= 1 || eligible.len() < floor;
            if fail_open {
                (candidates.clone(), false)
            } else {
                (eligible, view.config.latency_weighting)
            }
        }
        None => (candidates.clone(), false),
    };

    let weight_of = |backend: &Backend, addr: SocketAddr| -> u64 {
        let configured = resolve_backend_weight(backend);
        if use_latency {
            if let Some(view) = health {
                if let Some(state) = view
                    .table
                    .get(&BackendKey::new(pool_name.to_string(), addr))
                {
                    return effective_weight(configured, state.weight_factor());
                }
            }
        }
        configured as u64
    };

    let total: u64 = chosen.iter().map(|(b, addr)| weight_of(b, *addr)).sum();
    if total == 0 {
        return None;
    }

    let pick = (txn_id.wrapping_add(snapshot_generation)) % total;
    let mut acc = 0u64;
    for (backend, addr) in &chosen {
        acc += weight_of(backend, *addr);
        if pick < acc {
            return Some((pool_name.to_string(), *addr));
        }
    }
    chosen
        .last()
        .map(|(_, addr)| (pool_name.to_string(), *addr))
}

/// Per-backend effective weights Route would use on a first attempt when health
/// is enabled for the pool. Ineligible backends read zero unless the pool is
/// failing open, in which case configured weights are returned for all backends.
pub fn effective_weights_for_scrape(
    pool: &Pool,
    pool_health: &CompiledPoolHealth,
    table: &HealthTable,
) -> std::collections::HashMap<SocketAddr, u64> {
    use std::collections::HashMap;
    let mut out = HashMap::new();
    let candidates: Vec<(&Backend, SocketAddr)> = pool
        .backends
        .iter()
        .filter_map(|backend| {
            let addr = backend.address.parse::<SocketAddr>().ok()?;
            Some((backend, addr))
        })
        .collect();
    if candidates.is_empty() {
        return out;
    }

    let eligible: Vec<(&Backend, SocketAddr)> = candidates
        .iter()
        .copied()
        .filter(|(_, addr)| backend_eligible(table, &pool.name, *addr))
        .collect();
    let floor = (pool_health.min_eligible.max(1)) as usize;
    let fail_open = pool.backends.len() <= 1 || eligible.len() < floor;
    let use_latency = !fail_open && pool_health.latency_weighting;

    for (backend, addr) in &candidates {
        let configured = resolve_backend_weight(backend);
        let weight = if fail_open {
            configured as u64
        } else if !backend_eligible(table, &pool.name, *addr) {
            0
        } else if use_latency {
            table
                .get(&BackendKey::new(pool.name.clone(), *addr))
                .map(|state| effective_weight(configured, state.weight_factor()))
                .unwrap_or(configured as u64)
        } else {
            configured as u64
        };
        out.insert(*addr, weight);
    }
    out
}

pub fn default_pool_name(cfg: &Config) -> Option<String> {
    if let Some(pool) = cfg.pools.iter().find(|p| p.name == "default") {
        return Some(pool.name.clone());
    }
    cfg.pools.first().map(|p| p.name.clone())
}

/// Metric/log label for a backend: configured `name` when set, else `address`.
pub fn backend_metric_label(backend: &Backend) -> String {
    backend
        .name
        .as_ref()
        .filter(|n| !n.is_empty())
        .cloned()
        .unwrap_or_else(|| backend.address.clone())
}

/// Metric/log label for a listener: configured `name` when set, else bind `address`.
///
/// Mirrors [`backend_metric_label`] so listener and backend labels follow the
/// same name-when-set convention.
pub fn listener_metric_label(listener: &Listener) -> String {
    listener
        .name
        .as_ref()
        .filter(|n| !n.is_empty())
        .cloned()
        .unwrap_or_else(|| listener.address.clone())
}

/// Resolve metric label for a backend address within a pool.
pub fn backend_metric_label_for_addr(pools: &[Pool], pool_name: &str, addr: SocketAddr) -> String {
    pools
        .iter()
        .find(|p| p.name == pool_name)
        .and_then(|pool| {
            pool.backends.iter().find_map(|b| {
                b.address
                    .parse::<SocketAddr>()
                    .ok()
                    .filter(|a| a == &addr)
                    .map(|_| backend_metric_label(b))
            })
        })
        .unwrap_or_else(|| addr.to_string())
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
                    name: None,
                    ..Default::default()
                },
                Backend {
                    address: "10.0.0.2:53".into(),
                    weight: Some(100),
                    name: None,
                    ..Default::default()
                },
                Backend {
                    address: "10.0.0.3:53".into(),
                    weight: Some(100),
                    name: None,
                    ..Default::default()
                },
            ],
            sources_v4: vec![],
            sources_v6: vec![],
            max_inflight: None,
            health: None,
        }
    }

    #[test]
    fn backend_metric_label_prefers_name() {
        use conduit_proto::config::Backend;
        let b = Backend {
            address: "127.0.0.1:5300".into(),
            weight: Some(100),
            name: Some("resolver-east".into()),
            ..Default::default()
        };
        assert_eq!(backend_metric_label(&b), "resolver-east");
        assert_eq!(
            backend_metric_label(&Backend {
                address: "127.0.0.1:5300".into(),
                weight: Some(100),
                name: None,
                ..Default::default()
            }),
            "127.0.0.1:5300"
        );
    }

    #[test]
    fn listener_metric_label_prefers_name() {
        use conduit_proto::config::Listener;
        let named = Listener {
            address: "127.0.0.1:15353".into(),
            protocol: "udp".into(),
            threads: None,
            reuse_port: None,
            name: Some("lab-udp".into()),
            rcvbuf: None,
        };
        assert_eq!(listener_metric_label(&named), "lab-udp");

        let unnamed = Listener {
            name: None,
            ..named.clone()
        };
        assert_eq!(listener_metric_label(&unnamed), "127.0.0.1:15353");

        let empty_name = Listener {
            name: Some(String::new()),
            ..named
        };
        assert_eq!(listener_metric_label(&empty_name), "127.0.0.1:15353");
    }

    #[test]
    fn first_attempt_ignores_tried_list() {
        let pools = vec![pool_three_backends()];
        let tried = vec!["10.0.0.1:53".parse().unwrap()];
        let (_, addr) = select_backend(&pools, "primary", 5, 0, 0, &tried, None).unwrap();
        assert_eq!(addr, "10.0.0.1:53".parse().unwrap());
    }

    #[test]
    fn retry_excludes_tried_backend_in_pool() {
        let pools = vec![pool_three_backends()];
        let b1 = "10.0.0.1:53".parse().unwrap();
        let (_, first) = select_backend(&pools, "primary", 2, 0, 0, &[], None).unwrap();
        let (_, second) = select_backend(&pools, "primary", 2, 0, 1, &[first], None).unwrap();
        assert_ne!(first, second);
        assert_eq!(first, b1);
        let (_, third) =
            select_backend(&pools, "primary", 2, 0, 2, &[first, second], None).unwrap();
        assert_ne!(third, first);
        assert_ne!(third, second);
    }

    #[test]
    fn retry_returns_none_when_pool_exhausted() {
        let pools = vec![pool_three_backends()];
        let b1 = "10.0.0.1:53".parse().unwrap();
        let b2 = "10.0.0.2:53".parse().unwrap();
        let b3 = "10.0.0.3:53".parse().unwrap();
        assert!(select_backend(&pools, "primary", 2, 0, 3, &[b1, b2, b3], None).is_none());
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
                    name: None,
                    ..Default::default()
                }],
                sources_v4: vec![],
                sources_v6: vec![],
                max_inflight: None,
                health: None,
            },
        ];
        let primary_tried = vec!["10.0.0.1:53".parse().unwrap()];
        let (_, addr) = select_backend(&pools, "secondary", 7, 0, 1, &primary_tried, None).unwrap();
        assert_eq!(addr, "10.0.1.1:53".parse().unwrap());
    }

    // ---- Phase B: health-aware selection (eligibility, weight, fail-open) ----

    use crate::health::{BackendHealthState, BackendKey, HealthTable};
    use conduit_config::health::{CompiledPoolHealth, InitialHealthState};
    use std::sync::Arc;

    fn pool_health(latency_weighting: bool, min_eligible: u32) -> CompiledPoolHealth {
        CompiledPoolHealth {
            interval_ms: 1000,
            timeout_ms: 1000,
            rise: 3,
            fall: 2,
            acceptable_rcodes: None,
            initial_state: InitialHealthState::Optimistic,
            latency_weighting,
            latency_ewma_alpha: 0.2,
            latency_floor: 0.25,
            min_eligible,
            passive_fast_trip: true,
            passive_fall: 2,
            backends: vec![],
        }
    }

    /// Build a health table for `pool` seeding every backend Up (eligible).
    fn table_all_up(pool: &Pool) -> HealthTable {
        let mut table = HealthTable::new();
        for b in &pool.backends {
            let addr: SocketAddr = b.address.parse().unwrap();
            table.insert(
                BackendKey::new(pool.name.clone(), addr),
                Arc::new(BackendHealthState::from_initial_policy(
                    InitialHealthState::Optimistic,
                )),
            );
        }
        table
    }

    fn set_down(table: &HealthTable, pool: &str, addr: &str) {
        table
            .get(&BackendKey::new(pool.to_string(), addr.parse().unwrap()))
            .unwrap()
            .set_down();
    }

    /// Distribution of selected backends over many transactions.
    fn distribution(
        pools: &[Pool],
        pool_name: &str,
        health: Option<PoolHealthView<'_>>,
    ) -> std::collections::HashMap<SocketAddr, u32> {
        let mut counts = std::collections::HashMap::new();
        for txn in 0..3000u64 {
            if let Some((_, addr)) = select_backend(pools, pool_name, txn, 0, 0, &[], health) {
                *counts.entry(addr).or_insert(0) += 1;
            }
        }
        counts
    }

    #[test]
    fn eligibility_filter_excludes_down_backend() {
        let pools = vec![pool_three_backends()];
        let table = table_all_up(&pools[0]);
        set_down(&table, "primary", "10.0.0.2:53");
        let config = pool_health(false, 0);
        let view = PoolHealthView {
            config: &config,
            table: &table,
        };
        let counts = distribution(&pools, "primary", Some(view));
        let down: SocketAddr = "10.0.0.2:53".parse().unwrap();
        assert_eq!(
            counts.get(&down),
            None,
            "down backend must receive no traffic"
        );
        assert!(counts.len() == 2, "only the two up backends are selected");
    }

    #[test]
    fn health_none_considers_all_backends() {
        let pools = vec![pool_three_backends()];
        let counts = distribution(&pools, "primary", None);
        assert_eq!(counts.len(), 3, "no health config => all backends eligible");
    }

    #[test]
    fn latency_skew_shifts_share_but_never_above_configured() {
        // Two equal-weight backends; the slower one's factor is reduced.
        let mut pool = pool_three_backends();
        pool.backends.truncate(2);
        let pools = vec![pool];
        let table = table_all_up(&pools[0]);
        // Backend 2 is half-weight via latency factor; backend 1 stays at 1.0.
        table
            .get(&BackendKey::new(
                "primary",
                "10.0.0.2:53".parse::<SocketAddr>().unwrap(),
            ))
            .unwrap()
            .damp_weight_factor(0.5, 1.0);
        let config = pool_health(true, 0);
        let view = PoolHealthView {
            config: &config,
            table: &table,
        };
        let counts = distribution(&pools, "primary", Some(view));
        let fast = counts[&"10.0.0.1:53".parse().unwrap()];
        let slow = counts[&"10.0.0.2:53".parse().unwrap()];
        assert!(
            fast > slow,
            "lower-latency backend gets a larger share: fast={fast} slow={slow}"
        );
        // 100 vs 50 → roughly 2:1; fast must not exceed its configured share
        // (latency only reduces the slow one, never inflates the fast one).
        let ratio = fast as f64 / slow as f64;
        assert!(ratio > 1.5 && ratio < 2.5, "ratio {ratio} not ~2:1");
    }

    #[test]
    fn latency_factor_clamped_to_floor() {
        // Even an extreme target cannot drop a backend below the floor.
        let s = BackendHealthState::from_initial_policy(InitialHealthState::Optimistic);
        s.damp_weight_factor(0.0, 1.0); // snap toward 0
        assert!(s.weight_factor() < 0.0001);
        // effective_weight floors the integer weight at 1 (eligible never zeroed).
        assert_eq!(effective_weight(100, s.weight_factor()), 1);
        assert_eq!(effective_weight(100, 1.0), 100);
        assert_eq!(effective_weight(100, 0.25), 25);
    }

    #[test]
    fn damp_weight_factor_moves_partway() {
        let s = BackendHealthState::from_initial_policy(InitialHealthState::Optimistic);
        assert_eq!(s.weight_factor(), 1.0);
        let next = s.damp_weight_factor(0.25, 0.5); // halfway: 1.0 -> 0.625
        assert!((next - 0.625).abs() < 1e-9, "got {next}");
        let next2 = s.damp_weight_factor(0.25, 0.5); // 0.625 -> 0.4375
        assert!((next2 - 0.4375).abs() < 1e-9, "got {next2}");
        assert!(next2 > 0.25, "still damping toward target, not jumping");
    }

    #[test]
    fn fail_open_when_all_down() {
        let pools = vec![pool_three_backends()];
        let table = table_all_up(&pools[0]);
        set_down(&table, "primary", "10.0.0.1:53");
        set_down(&table, "primary", "10.0.0.2:53");
        set_down(&table, "primary", "10.0.0.3:53");
        let config = pool_health(false, 0); // floor effectively 1
        let view = PoolHealthView {
            config: &config,
            table: &table,
        };
        let counts = distribution(&pools, "primary", Some(view));
        assert_eq!(
            counts.values().sum::<u32>(),
            3000,
            "all-down must fail open, not SERVFAIL for lack of an eligible backend"
        );
        assert_eq!(counts.len(), 3, "fail open restores every backend");
    }

    #[test]
    fn fail_open_below_min_eligible_floor() {
        // Floor of 2: with only one backend up, Route ignores health.
        let pools = vec![pool_three_backends()];
        let table = table_all_up(&pools[0]);
        set_down(&table, "primary", "10.0.0.2:53");
        set_down(&table, "primary", "10.0.0.3:53");
        let config = pool_health(false, 2);
        let view = PoolHealthView {
            config: &config,
            table: &table,
        };
        let counts = distribution(&pools, "primary", Some(view));
        assert_eq!(counts.len(), 3, "below floor => all candidates eligible");
    }

    #[test]
    fn above_floor_routes_only_to_eligible() {
        // Floor of 1: two up backends is above the floor, so health applies.
        let pools = vec![pool_three_backends()];
        let table = table_all_up(&pools[0]);
        set_down(&table, "primary", "10.0.0.3:53");
        let config = pool_health(false, 1);
        let view = PoolHealthView {
            config: &config,
            table: &table,
        };
        let counts = distribution(&pools, "primary", Some(view));
        assert_eq!(
            counts.len(),
            2,
            "two eligible above floor => health applies"
        );
        assert_eq!(counts.get(&"10.0.0.3:53".parse().unwrap()), None);
    }

    #[test]
    fn single_backend_pool_always_fails_open() {
        let pool = Pool {
            name: "solo".into(),
            backends: vec![Backend {
                address: "10.0.9.9:53".into(),
                weight: Some(100),
                name: None,
                ..Default::default()
            }],
            sources_v4: vec![],
            sources_v6: vec![],
            max_inflight: None,
            health: None,
        };
        let pools = vec![pool];
        let table = table_all_up(&pools[0]);
        set_down(&table, "solo", "10.0.9.9:53");
        let config = pool_health(false, 0);
        let view = PoolHealthView {
            config: &config,
            table: &table,
        };
        let (_, addr) = select_backend(&pools, "solo", 1, 0, 0, &[], Some(view)).unwrap();
        assert_eq!(addr, "10.0.9.9:53".parse::<SocketAddr>().unwrap());
    }
}
