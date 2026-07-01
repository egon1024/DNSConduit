//! Scrape-time health metric rows (phase 1c §10, backend-health-metrics delta).

use super::control::EffectiveScope;
use super::state::{BackendKey, Health, HealthRegistry};
use crate::routing::{backend_metric_label, effective_weights_for_scrape};
use conduit_config::health::CompiledHealth;
use conduit_proto::config::Config;

/// One backend row for Prometheus health gauges.
#[derive(Debug, Clone, PartialEq)]
pub struct BackendHealthScrapeRow {
    pub pool: String,
    pub backend: String,
    pub observed: f64,
    pub applied: f64,
    /// `1.0` when probe-driven transitions apply; `0.0` when frozen.
    pub probe_automatic: f64,
    pub effective_weight: f64,
    /// Latency EWMA in milliseconds; omitted from export when `None`.
    pub latency_ewma_ms: Option<f64>,
    pub transitions_total: u64,
}

/// Build health scrape rows and per-pool active backend counts for pools with
/// health checking enabled.
pub fn build_health_scrape(
    config: &Config,
    compiled: &CompiledHealth,
    registry: &HealthRegistry,
) -> (Vec<BackendHealthScrapeRow>, Vec<(String, u32)>) {
    registry.sync_frozen_flags(compiled);
    let table = registry.load();
    let mut backends = Vec::new();
    let mut pool_active = Vec::new();

    for pool in &config.pools {
        let Some(pool_health) = compiled.pool(&pool.name) else {
            continue;
        };
        let weights = effective_weights_for_scrape(pool, pool_health, &table);
        let mut active = 0u32;
        for backend in &pool.backends {
            let Ok(addr) = backend.address.parse() else {
                continue;
            };
            let key = BackendKey::new(pool.name.clone(), addr);
            let Some(state) = table.get(&key) else {
                continue;
            };
            let applied = state.applied();
            if applied == Health::Up {
                active += 1;
            }
            let scope = registry.resolve_scope(&pool.name, addr);
            backends.push(BackendHealthScrapeRow {
                pool: pool.name.clone(),
                backend: backend_metric_label(backend),
                observed: state.observed().as_metric_value(),
                applied: applied.as_metric_value(),
                probe_automatic: if scope == EffectiveScope::Automatic {
                    1.0
                } else {
                    0.0
                },
                effective_weight: weights.get(&addr).copied().unwrap_or(0) as f64,
                latency_ewma_ms: state.latency_ewma_ms(),
                transitions_total: state.transitions_total(),
            });
        }
        pool_active.push((pool.name.clone(), active));
    }

    (backends, pool_active)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::health::HealthControlAction;
    use crate::health::HealthControlScope;
    use crate::RuntimeSnapshot;
    use conduit_config::load_yaml;

    fn health_fixture() -> (Config, CompiledHealth, HealthRegistry) {
        let yaml = include_str!("../../../../tests/fixtures/config/with-health.yaml");
        let cfg = load_yaml(yaml).unwrap();
        let snap = RuntimeSnapshot::from_config(cfg.clone());
        (
            cfg,
            snap.health.clone(),
            HealthRegistry::from_compiled(&snap.health),
        )
    }

    #[test]
    fn down_backend_has_zero_effective_weight() {
        let (cfg, compiled, registry) = health_fixture();
        let addr: std::net::SocketAddr = "127.0.0.1:5300".parse().unwrap();
        registry.get("default", addr).unwrap().set_down();
        let (rows, active) = build_health_scrape(&cfg, &compiled, &registry);
        let row = rows.iter().find(|r| r.backend == "127.0.0.1:5300").unwrap();
        assert_eq!(row.applied, 2.0);
        assert_eq!(row.effective_weight, 0.0);
        assert_eq!(active, vec![("default".into(), 1)]);
    }

    #[test]
    fn fail_open_restores_configured_effective_weights() {
        let (cfg, compiled, registry) = health_fixture();
        let b0: std::net::SocketAddr = "127.0.0.1:5300".parse().unwrap();
        let b1: std::net::SocketAddr = "127.0.0.1:5301".parse().unwrap();
        registry.get("default", b0).unwrap().set_down();
        registry.get("default", b1).unwrap().set_down();
        let (rows, _) = build_health_scrape(&cfg, &compiled, &registry);
        for row in &rows {
            assert_eq!(row.effective_weight, 100.0, "panic fail-open: {row:?}");
        }
    }

    #[test]
    fn frozen_drain_shows_observed_up_applied_down() {
        let (cfg, compiled, registry) = health_fixture();
        let addr: std::net::SocketAddr = "127.0.0.1:5300".parse().unwrap();
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
        let (rows, _) = build_health_scrape(&cfg, &compiled, &registry);
        let row = rows.iter().find(|r| r.backend == "127.0.0.1:5300").unwrap();
        assert_eq!(row.observed, 1.0);
        assert_eq!(row.applied, 2.0);
        assert_eq!(row.probe_automatic, 0.0);
    }

    #[test]
    fn cardinality_is_pool_backend_only() {
        let (cfg, compiled, registry) = health_fixture();
        let (rows, active) = build_health_scrape(&cfg, &compiled, &registry);
        assert_eq!(rows.len(), 2);
        assert!(rows
            .iter()
            .all(|r| !r.pool.is_empty() && !r.backend.is_empty()));
        assert_eq!(active.len(), 1);
    }
}
