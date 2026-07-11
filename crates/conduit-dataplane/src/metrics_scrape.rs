//! Build scrape-time gauge snapshot from snapshot store, txn table, and slot pool.

use crate::forward::TxnTable;
use conduit_config::resolve_listener_ingress;
use conduit_core::health::build_health_scrape;
use conduit_core::routing::{backend_metric_label, listener_metric_label, resolve_backend_weight};
use conduit_core::txn_store::SharedTxnStore;
use conduit_core::SnapshotStore;
use conduit_metrics::{
    ip_family_label, BackendIdentity, HealthScrapeBackend, ListenerIdentity, ScrapeGaugeSnapshot,
};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;

pub fn build_scrape_snapshot(
    store: &SnapshotStore,
    table: &TxnTable,
    txn_store: &SharedTxnStore,
) -> ScrapeGaugeSnapshot {
    let snap = store.load();
    let registry = store.health();
    let (health_rows, pool_active) =
        build_health_scrape(&snap.config, &snap.health, registry.as_ref());
    let health_backends = health_rows
        .into_iter()
        .map(|row| HealthScrapeBackend {
            pool: row.pool,
            backend: row.backend,
            observed: row.observed,
            applied: row.applied,
            probe_automatic: row.probe_automatic,
            effective_weight: row.effective_weight,
            latency_ewma_ms: row.latency_ewma_ms,
            transitions_total: row.transitions_total,
        })
        .collect();
    let outstanding: HashMap<SocketAddr, u32> =
        table.outstanding_per_backend().into_iter().collect();
    let slot_stats = {
        let store = txn_store.lock();
        (store.in_use(), store.capacity(), store.exhaustion_total())
    };

    let mut pool_backend_counts = Vec::new();
    let mut forward_outstanding = Vec::new();
    let mut backends = Vec::new();

    for pool in &snap.config.pools {
        pool_backend_counts.push((pool.name.clone(), pool.backends.len() as u32));
        for b in &pool.backends {
            let label = backend_metric_label(b);
            backends.push(BackendIdentity {
                pool: pool.name.clone(),
                label: label.clone(),
                address: b.address.clone(),
                name: b.name.clone().unwrap_or_default(),
                weight: resolve_backend_weight(b),
            });
            let Ok(addr) = b.address.parse::<SocketAddr>() else {
                continue;
            };
            let count = outstanding.get(&addr).copied().unwrap_or(0);
            forward_outstanding.push((pool.name.clone(), label, count));
        }
    }

    let listeners = snap
        .config
        .listeners
        .as_ref()
        .map(|block| {
            block
                .listeners
                .iter()
                .map(|ln| {
                    let resolved = resolve_listener_ingress(block, ln);
                    let ip_family = ln
                        .address
                        .parse::<SocketAddr>()
                        .map(|addr| ip_family_label(&addr).to_string())
                        .unwrap_or_else(|_| "unknown".into());
                    ListenerIdentity {
                        label: listener_metric_label(ln),
                        address: ln.address.clone(),
                        name: ln.name.clone().unwrap_or_default(),
                        protocol: ln.protocol.to_lowercase(),
                        ip_family,
                        reuse_port: resolved.reuse_port,
                        threads: resolved.threads,
                        rcvbuf: resolved.rcvbuf,
                    }
                })
                .collect()
        })
        .unwrap_or_default();

    ScrapeGaugeSnapshot {
        config_generation: snap.generation,
        pool_backend_counts,
        forward_outstanding,
        slots_in_use: slot_stats.0,
        slots_capacity: slot_stats.1,
        slot_pool_exhausted_total: slot_stats.2,
        listeners,
        backends,
        health_backends,
        pool_backends_active: pool_active,
        cache_entry_counts: Vec::new(),
    }
}

pub fn scrape_snapshot_fn(
    store: Arc<SnapshotStore>,
    table: Arc<TxnTable>,
    txn_store: SharedTxnStore,
) -> Arc<dyn Fn() -> ScrapeGaugeSnapshot + Send + Sync> {
    Arc::new(move || {
        let mut snap = build_scrape_snapshot(&store, &table, &txn_store);
        snap.cache_entry_counts = store.cache().all_entry_counts();
        snap
    })
}

pub fn scrape_snapshot_fn_with_cache(
    store: Arc<SnapshotStore>,
    table: Arc<TxnTable>,
    txn_store: SharedTxnStore,
    cache: Option<Arc<conduit_core::lookup::LookupCacheRegistry>>,
) -> Arc<dyn Fn() -> ScrapeGaugeSnapshot + Send + Sync> {
    Arc::new(move || {
        let mut snap = build_scrape_snapshot(&store, &table, &txn_store);
        if let Some(cache) = cache.as_ref() {
            snap.cache_entry_counts = cache.all_entry_counts();
        }
        snap
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use conduit_config::load_yaml;
    use conduit_core::RuntimeSnapshot;

    #[test]
    fn forward_outstanding_includes_zero_for_configured_backends() {
        let yaml = include_str!("../../../tests/fixtures/config/with-metrics-prometheus.yaml");
        let cfg = load_yaml(yaml).unwrap();
        let snap = RuntimeSnapshot::from_config(cfg);
        let store = SnapshotStore::new(snap);
        let table = TxnTable::new(64, 50);
        let txn_store = conduit_core::txn_store::SharedTxnStore::new(64, 256);
        let shot = build_scrape_snapshot(&store, &table, &txn_store);
        assert_eq!(shot.pool_backend_counts, vec![("default".to_string(), 1)]);
        assert_eq!(shot.forward_outstanding.len(), 1);
        assert_eq!(shot.forward_outstanding[0].2, 0);
        assert_eq!(shot.forward_outstanding[0].1, "127.0.0.1:15300");
        assert_eq!(shot.slots_in_use, 0);
        assert_eq!(shot.slots_capacity, 64);
        assert_eq!(shot.slot_pool_exhausted_total, 0);
    }

    #[test]
    fn forward_outstanding_uses_backend_name_when_set() {
        let yaml = include_str!("../../../tests/fixtures/config/dataplane-named-backends.yaml");
        let cfg = load_yaml(yaml).unwrap();
        let snap = RuntimeSnapshot::from_config(cfg);
        let store = SnapshotStore::new(snap);
        let table = TxnTable::new(64, 50);
        let txn_store = conduit_core::txn_store::SharedTxnStore::new(64, 256);
        let shot = build_scrape_snapshot(&store, &table, &txn_store);
        assert_eq!(shot.forward_outstanding.len(), 1);
        assert_eq!(shot.forward_outstanding[0].1, "resolver-east");
    }

    #[test]
    fn identity_rows_capture_listener_and_backend_name_and_address() {
        let yaml = include_str!("../../../tests/fixtures/config/dataplane-named-backends.yaml");
        let cfg = load_yaml(yaml).unwrap();
        let snap = RuntimeSnapshot::from_config(cfg);
        let store = SnapshotStore::new(snap);
        let table = TxnTable::new(64, 50);
        let txn_store = conduit_core::txn_store::SharedTxnStore::new(64, 256);
        let shot = build_scrape_snapshot(&store, &table, &txn_store);

        // Unnamed listener: label and address are the bind address, name empty,
        // with resolved ingress settings and derived ip_family/protocol.
        assert_eq!(shot.listeners.len(), 1);
        let ln = &shot.listeners[0];
        assert_eq!(ln.label, "127.0.0.1:15353");
        assert_eq!(ln.address, "127.0.0.1:15353");
        assert_eq!(ln.name, "");
        assert_eq!(ln.protocol, "udp");
        assert_eq!(ln.ip_family, "v4");
        assert_eq!(ln.threads, 1);

        // Named backend: label/name are the name, address is the ip:port, with
        // the effective weight (default 100 when unset).
        assert_eq!(shot.backends.len(), 1);
        let b = &shot.backends[0];
        assert_eq!(b.pool, "default");
        assert_eq!(b.label, "resolver-east");
        assert_eq!(b.address, "127.0.0.1:5300");
        assert_eq!(b.name, "resolver-east");
        assert_eq!(b.weight, 100);
    }

    #[test]
    fn forward_outstanding_reflects_txn_table() {
        use crate::forward::{ForwardKey, TxnTable};

        let yaml = include_str!("../../../tests/fixtures/config/with-metrics-prometheus.yaml");
        let cfg = load_yaml(yaml).unwrap();
        let snap = RuntimeSnapshot::from_config(cfg);
        let store = SnapshotStore::new(snap);
        let table = TxnTable::new(64, 50);
        let txn_store = conduit_core::txn_store::SharedTxnStore::new(64, 256);
        let backend: std::net::SocketAddr = "127.0.0.1:15300".parse().unwrap();
        let key = ForwardKey {
            backend,
            dns_id: 42,
        };
        assert!(table.register(key, 1));
        let shot = build_scrape_snapshot(&store, &table, &txn_store);
        assert_eq!(shot.forward_outstanding[0].2, 1);
    }

    #[test]
    fn health_scrape_includes_series_for_enabled_pool() {
        let yaml = include_str!("../../../tests/fixtures/config/with-health.yaml");
        let cfg = load_yaml(yaml).unwrap();
        let snap = RuntimeSnapshot::from_config(cfg);
        let store = SnapshotStore::new(snap);
        let table = TxnTable::new(64, 50);
        let txn_store = conduit_core::txn_store::SharedTxnStore::new(64, 256);
        let shot = build_scrape_snapshot(&store, &table, &txn_store);
        assert_eq!(shot.health_backends.len(), 2);
        assert_eq!(shot.pool_backends_active, vec![("default".to_string(), 2)]);
    }

    #[test]
    fn health_scrape_reflects_down_and_resume_cycle() {
        use conduit_core::health::{HealthControlAction, HealthControlScope};
        use conduit_metrics::{encode_builtin, BuiltinProfile, BuiltinRegistry};
        use std::sync::Arc;

        let yaml = include_str!("../../../tests/fixtures/config/with-health.yaml");
        let cfg = load_yaml(yaml).unwrap();
        let snap = RuntimeSnapshot::from_config(cfg);
        let store = SnapshotStore::new(snap);
        let table = TxnTable::new(64, 50);
        let txn_store = conduit_core::txn_store::SharedTxnStore::new(64, 256);
        let store = Arc::new(store);
        let table = Arc::new(table);
        let txn_store = txn_store.clone();

        let reg = BuiltinRegistry::new(true, BuiltinProfile::Full);
        reg.set_scrape_snapshot_fn(crate::metrics_scrape::scrape_snapshot_fn(
            store.clone(),
            table.clone(),
            txn_store,
        ));

        let addr: std::net::SocketAddr = "127.0.0.1:5300".parse().unwrap();
        let registry = store.health();
        let compiled = store.load().health.clone();

        let body_up = encode_builtin(reg.gather());
        assert!(
            body_up.contains(
                r#"conduit_backend_health_applied{backend="127.0.0.1:5300",pool="default"} 1"#
            ),
            "optimistic initial applied=up, body:\n{body_up}"
        );

        registry.get("default", addr).unwrap().set_down();
        registry
            .get("default", addr)
            .unwrap()
            .record_success(1, 0.2, 1.0);
        let body_down = encode_builtin(reg.gather());
        assert!(
            body_down.contains(
                r#"conduit_backend_health_observed{backend="127.0.0.1:5300",pool="default"} 1"#
            ),
            "frozen drain: observed stays up, body:\n{body_down}"
        );
        assert!(
            body_down.contains(
                r#"conduit_backend_health_applied{backend="127.0.0.1:5300",pool="default"} 2"#
            ),
            "body:\n{body_down}"
        );
        assert!(
            body_down.contains(
                r#"conduit_backend_health_effective_weight{backend="127.0.0.1:5300",pool="default"} 0"#
            ),
            "body:\n{body_down}"
        );
        assert!(
            body_down.contains(r#"conduit_pool_backends_active{pool="default"} 1"#),
            "body:\n{body_down}"
        );

        registry
            .set_health_control(
                &compiled,
                HealthControlScope::Backend {
                    pool: "default".into(),
                    address: addr,
                },
                HealthControlAction::ResumeAutomatic,
            )
            .unwrap();

        let body_resume = encode_builtin(reg.gather());
        assert!(
            body_resume.contains(
                r#"conduit_backend_health_applied{backend="127.0.0.1:5300",pool="default"} 1"#
            ),
            "resume snaps to observed up, body:\n{body_resume}"
        );
        assert!(
            body_resume.contains(r#"conduit_pool_backends_active{pool="default"} 2"#),
            "body:\n{body_resume}"
        );
    }
}
