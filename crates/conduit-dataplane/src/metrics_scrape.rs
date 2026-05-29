//! Build scrape-time gauge snapshot from snapshot store and txn table.

use crate::forward::TxnTable;
use conduit_core::SnapshotStore;
use conduit_metrics::ScrapeGaugeSnapshot;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;

pub fn build_scrape_snapshot(store: &SnapshotStore, table: &TxnTable) -> ScrapeGaugeSnapshot {
    let snap = store.load();
    let outstanding: HashMap<SocketAddr, u32> =
        table.outstanding_per_backend().into_iter().collect();

    let mut pool_backend_counts = Vec::new();
    let mut forward_outstanding = Vec::new();

    for pool in &snap.config.pools {
        pool_backend_counts.push((pool.name.clone(), pool.backends.len() as u32));
        for b in &pool.backends {
            let Ok(addr) = b.address.parse::<SocketAddr>() else {
                continue;
            };
            let count = outstanding.get(&addr).copied().unwrap_or(0);
            forward_outstanding.push((pool.name.clone(), addr.to_string(), count));
        }
    }

    ScrapeGaugeSnapshot {
        config_generation: snap.generation,
        pool_backend_counts,
        forward_outstanding,
    }
}

pub fn scrape_snapshot_fn(
    store: Arc<SnapshotStore>,
    table: Arc<TxnTable>,
) -> Arc<dyn Fn() -> ScrapeGaugeSnapshot + Send + Sync> {
    Arc::new(move || build_scrape_snapshot(&store, &table))
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
        let shot = build_scrape_snapshot(&store, &table);
        assert_eq!(shot.pool_backend_counts, vec![("default".to_string(), 1)]);
        assert_eq!(shot.forward_outstanding.len(), 1);
        assert_eq!(shot.forward_outstanding[0].2, 0);
        assert!(shot.forward_outstanding[0].1.contains("15300"));
    }
}
