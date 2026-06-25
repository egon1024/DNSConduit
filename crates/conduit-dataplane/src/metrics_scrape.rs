//! Build scrape-time gauge snapshot from snapshot store, txn table, and slot pool.

use crate::forward::TxnTable;
use conduit_core::routing::backend_metric_label;
use conduit_core::txn_store::SharedTxnStore;
use conduit_core::SnapshotStore;
use conduit_metrics::ScrapeGaugeSnapshot;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;

pub fn build_scrape_snapshot(
    store: &SnapshotStore,
    table: &TxnTable,
    txn_store: &SharedTxnStore,
) -> ScrapeGaugeSnapshot {
    let snap = store.load();
    let outstanding: HashMap<SocketAddr, u32> =
        table.outstanding_per_backend().into_iter().collect();
    let slot_stats = {
        let store = txn_store.lock();
        (store.in_use(), store.capacity(), store.exhaustion_total())
    };

    let mut pool_backend_counts = Vec::new();
    let mut forward_outstanding = Vec::new();

    for pool in &snap.config.pools {
        pool_backend_counts.push((pool.name.clone(), pool.backends.len() as u32));
        for b in &pool.backends {
            let Ok(addr) = b.address.parse::<SocketAddr>() else {
                continue;
            };
            let label = backend_metric_label(b);
            let count = outstanding.get(&addr).copied().unwrap_or(0);
            forward_outstanding.push((pool.name.clone(), label, count));
        }
    }

    ScrapeGaugeSnapshot {
        config_generation: snap.generation,
        pool_backend_counts,
        forward_outstanding,
        slots_in_use: slot_stats.0,
        slots_capacity: slot_stats.1,
        slot_pool_exhausted_total: slot_stats.2,
    }
}

pub fn scrape_snapshot_fn(
    store: Arc<SnapshotStore>,
    table: Arc<TxnTable>,
    txn_store: SharedTxnStore,
) -> Arc<dyn Fn() -> ScrapeGaugeSnapshot + Send + Sync> {
    Arc::new(move || build_scrape_snapshot(&store, &table, &txn_store))
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
}
