//! Build `RoutingRuntimeSnapshot` for Rhai `runtime.routing` (design §D3).

use conduit_config::health::CompiledHealth;
use conduit_proto::config::Config;
use conduit_script::{BackendRoutingView, PoolRoutingView, RoutingRuntimeSnapshot};
use std::collections::HashMap;
use std::net::SocketAddr;

use crate::health::control::EffectiveScope;
use crate::health::state::{BackendKey, Health, HealthRegistry, HealthTable};
use crate::routing::backend_metric_label;

fn health_label(h: Health) -> &'static str {
    match h {
        Health::Up => "up",
        Health::Down => "down",
        Health::Unknown => "unknown",
    }
}

fn backend_eligible(table: &HealthTable, pool: &str, addr: SocketAddr) -> bool {
    match table.get(&BackendKey::new(pool.to_string(), addr)) {
        Some(state) => state.applied() == Health::Up,
        None => true,
    }
}

fn pool_fail_open(
    pool_name: &str,
    pool_backend_count: usize,
    pool_health: &conduit_config::health::CompiledPoolHealth,
    table: &HealthTable,
    candidates: &[SocketAddr],
) -> bool {
    let eligible: usize = candidates
        .iter()
        .filter(|addr| backend_eligible(table, pool_name, **addr))
        .count();
    let floor = (pool_health.min_eligible.max(1)) as usize;
    pool_backend_count <= 1 || eligible < floor
}

/// Build a routing runtime snapshot from health state, pool config, and outstanding
/// forward counts. Values reflect host state at hook entry on this worker.
pub fn build_routing_runtime_snapshot(
    config: &Config,
    compiled: &CompiledHealth,
    registry: &HealthRegistry,
    outstanding: &HashMap<SocketAddr, u32>,
    config_generation: u64,
) -> RoutingRuntimeSnapshot {
    registry.sync_frozen_flags(compiled);
    let table = registry.load();
    let mut pools = HashMap::new();
    let mut backends = HashMap::new();

    for pool in &config.pools {
        let pool_health = compiled.pool(&pool.name);
        let addrs: Vec<SocketAddr> = pool
            .backends
            .iter()
            .filter_map(|b| b.address.parse().ok())
            .collect();
        let configured_count = pool.backends.len() as u32;
        let eligible_count = if pool_health.is_some() {
            addrs
                .iter()
                .filter(|a| backend_eligible(&table, &pool.name, **a))
                .count() as u32
        } else {
            configured_count
        };
        let fail_open_active = pool_health
            .is_some_and(|ph| pool_fail_open(&pool.name, pool.backends.len(), ph, &table, &addrs));

        let mut min_latency: Option<f64> = None;
        let mut max_out = 0u32;
        if pool_health.is_some() {
            for backend in &pool.backends {
                let Ok(addr) = backend.address.parse::<SocketAddr>() else {
                    continue;
                };
                let key = BackendKey::new(pool.name.clone(), addr);
                let label = backend_metric_label(backend);
                let out = outstanding.get(&addr).copied().unwrap_or(0);
                max_out = max_out.max(out);

                let view = if let Some(state) = table.get(&key) {
                    let observed = state.observed();
                    let applied = state.applied();
                    let ewma = state.latency_ewma_ms();
                    if let Some(ms) = ewma {
                        min_latency = Some(min_latency.map_or(ms, |m| m.min(ms)));
                    }
                    BackendRoutingView {
                        configured: true,
                        applied: health_label(applied),
                        observed: health_label(observed),
                        eligible: applied == Health::Up,
                        frozen: registry.resolve_scope(&pool.name, addr) == EffectiveScope::Frozen,
                        latency_ewma_ms: ewma,
                        weight_factor: state.weight_factor(),
                        outstanding: out,
                        last_transition_unix_ms: state.last_transition_unix_ms(),
                    }
                } else {
                    BackendRoutingView {
                        configured: true,
                        applied: "up",
                        observed: "unknown",
                        eligible: true,
                        frozen: false,
                        latency_ewma_ms: None,
                        weight_factor: 1.0,
                        outstanding: out,
                        last_transition_unix_ms: None,
                    }
                };
                backends.insert((pool.name.clone(), label.clone()), view.clone());
                if label != backend.address {
                    backends.insert((pool.name.clone(), backend.address.clone()), view);
                }
            }
        } else {
            for backend in &pool.backends {
                let Ok(addr) = backend.address.parse::<SocketAddr>() else {
                    continue;
                };
                let label = backend_metric_label(backend);
                let out = outstanding.get(&addr).copied().unwrap_or(0);
                max_out = max_out.max(out);
                let view = BackendRoutingView {
                    configured: true,
                    applied: "up",
                    observed: "unknown",
                    eligible: true,
                    frozen: false,
                    latency_ewma_ms: None,
                    weight_factor: 1.0,
                    outstanding: out,
                    last_transition_unix_ms: None,
                };
                backends.insert((pool.name.clone(), label.clone()), view.clone());
                if label != backend.address {
                    backends.insert((pool.name.clone(), backend.address.clone()), view);
                }
            }
        }

        pools.insert(
            pool.name.clone(),
            PoolRoutingView {
                configured: true,
                configured_count,
                eligible_count,
                fail_open_active,
                min_latency_ewma_ms: min_latency,
                max_outstanding: max_out,
            },
        );
    }

    RoutingRuntimeSnapshot::new(config_generation, pools, backends)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::health::{HealthControlAction, HealthControlScope, HealthRegistry};
    use crate::RuntimeSnapshot;
    use conduit_config::load_yaml;

    fn fixture() -> (Config, CompiledHealth, HealthRegistry) {
        let yaml = include_str!("../../../tests/fixtures/config/with-health.yaml");
        let cfg = load_yaml(yaml).unwrap();
        let snap = RuntimeSnapshot::from_config(cfg.clone());
        (
            cfg,
            snap.health.clone(),
            HealthRegistry::from_compiled(&snap.health),
        )
    }

    #[test]
    fn down_backend_not_eligible_in_view() {
        let (cfg, compiled, registry) = fixture();
        let addr: SocketAddr = "127.0.0.1:5300".parse().unwrap();
        registry.get("default", addr).unwrap().set_down();
        let snap = build_routing_runtime_snapshot(&cfg, &compiled, &registry, &HashMap::new(), 1);
        let view = snap.backend("default", "127.0.0.1:5300");
        assert!(view.configured);
        assert!(!view.eligible);
        assert_eq!(view.applied, "down");
    }

    #[test]
    fn pool_eligible_count_matches_route() {
        let (cfg, compiled, registry) = fixture();
        let snap = build_routing_runtime_snapshot(&cfg, &compiled, &registry, &HashMap::new(), 1);
        let pool = snap.pool("default");
        assert_eq!(pool.eligible_count, 2);
        assert_eq!(pool.configured_count, 2);
    }

    #[test]
    fn unknown_pool_and_backend_return_empty_views() {
        let (cfg, compiled, registry) = fixture();
        let snap = build_routing_runtime_snapshot(&cfg, &compiled, &registry, &HashMap::new(), 1);
        assert!(!snap.pool("missing").configured);
        assert!(!snap.backend("default", "missing").configured);
        assert!(!snap.backend("missing", "127.0.0.1:5300").configured);
    }

    #[test]
    fn frozen_drain_shows_observed_up_applied_down() {
        let (cfg, compiled, registry) = fixture();
        let addr: SocketAddr = "127.0.0.1:5300".parse().unwrap();
        registry
            .get("default", addr)
            .unwrap()
            .record_success(1, 0.2, 1.0);
        registry
            .set_health_control(
                &compiled,
                HealthControlScope::Backend {
                    pool: "default".into(),
                    address: addr,
                },
                HealthControlAction::SetDown,
            )
            .unwrap();
        let snap = build_routing_runtime_snapshot(&cfg, &compiled, &registry, &HashMap::new(), 1);
        let view = snap.backend("default", "127.0.0.1:5300");
        assert_eq!(view.observed, "up");
        assert_eq!(view.applied, "down");
        assert!(view.frozen);
    }

    #[test]
    fn fail_open_when_all_down() {
        let (cfg, compiled, registry) = fixture();
        let b0: SocketAddr = "127.0.0.1:5300".parse().unwrap();
        let b1: SocketAddr = "127.0.0.1:5301".parse().unwrap();
        registry.get("default", b0).unwrap().set_down();
        registry.get("default", b1).unwrap().set_down();
        let snap = build_routing_runtime_snapshot(&cfg, &compiled, &registry, &HashMap::new(), 1);
        assert!(snap.pool("default").fail_open_active);
    }

    #[test]
    fn resolve_backend_by_name() {
        let yaml = include_str!("../../../tests/fixtures/config/dataplane-named-backends.yaml");
        let cfg = load_yaml(yaml).unwrap();
        let runtime = RuntimeSnapshot::from_config(cfg.clone());
        let registry = HealthRegistry::empty();
        let snap =
            build_routing_runtime_snapshot(&cfg, &runtime.health, &registry, &HashMap::new(), 0);
        let view = snap.backend("default", "resolver-east");
        assert!(view.configured);
    }
}
