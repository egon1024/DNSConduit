//! Route phase — select pool and weighted backend.

use crate::health::HealthRegistry;
use crate::phase::Phase;
use crate::pipeline::{PipelineStage, StageOutcome};
use crate::routing::{
    backend_metric_label_for_addr, default_pool_name, select_backend, tried_backends_in_pool,
    PoolHealthView,
};
use crate::snapshot::RuntimeSnapshot;
use crate::transaction::{ConvergenceReason, Transaction};
use conduit_config::defaults::DEFAULT_ROUTE_FAILURE_POLICY;
use std::sync::Arc;

/// Route phase. When `health` is set (the dataplane runtimes), selection is
/// health-aware (eligibility + effective weight + fail-open, design §D7); when
/// it is `None` (default, tests), selection is the pre-health weighted pick.
#[derive(Default)]
pub struct RouteStage {
    health: Option<Arc<HealthRegistry>>,
}

fn route_failure_continue_phase(snapshot: &RuntimeSnapshot) -> Phase {
    let policy = snapshot
        .config
        .orchestrator
        .as_ref()
        .map(|o| {
            if o.route_failure_policy.is_empty() {
                DEFAULT_ROUTE_FAILURE_POLICY
            } else {
                o.route_failure_policy.as_str()
            }
        })
        .unwrap_or(DEFAULT_ROUTE_FAILURE_POLICY);
    if policy == "response_rules" {
        Phase::ResponseRules
    } else {
        Phase::NoAnswer
    }
}

impl RouteStage {
    /// Health-unaware Route (today's behavior — all backends eligible).
    pub fn new() -> Self {
        Self::default()
    }

    /// Health-aware Route reading the runtime side-table lock-free at selection.
    pub fn with_health(health: Arc<HealthRegistry>) -> Self {
        Self {
            health: Some(health),
        }
    }
}

impl PipelineStage for RouteStage {
    fn name(&self) -> &'static str {
        "route"
    }

    fn handle(&self, txn: &mut Transaction, snapshot: &Arc<RuntimeSnapshot>) -> StageOutcome {
        let pool_name = if txn.attempt_count > 0 {
            txn.take_retry_pool().or_else(|| txn.selected_pool.clone())
        } else {
            txn.selected_pool.clone()
        }
        .or_else(|| default_pool_name(&snapshot.config));

        let Some(pool_name) = pool_name else {
            let continue_phase = route_failure_continue_phase(snapshot);
            tracing::warn!(
                txn_id = txn.id,
                reason = "no_pool",
                next_phase = ?continue_phase,
                "route failed: no pool resolvable"
            );
            txn.set_rcode_name("SERVFAIL");
            txn.set_convergence_reason(ConvergenceReason::NoPool);
            return StageOutcome::Continue(continue_phase);
        };

        // Read the lock-free health side-table for this pool, if health is wired
        // and enabled for the pool. The `Arc<HealthTable>` guard is held for the
        // duration of the selection call so the borrow stays valid.
        let table_guard = self.health.as_ref().map(|reg| reg.load());
        let health_view = match (&table_guard, snapshot.health.pool(&pool_name)) {
            (Some(table), Some(config)) => Some(PoolHealthView {
                config,
                table: table.as_ref(),
            }),
            _ => None,
        };

        let tried = tried_backends_in_pool(&txn.attempts, &pool_name);
        let Some((pool, backend)) = select_backend(
            &snapshot.config.pools,
            &pool_name,
            txn.id,
            snapshot.generation,
            txn.attempt_count,
            &tried,
            health_view,
        ) else {
            let continue_phase = route_failure_continue_phase(snapshot);
            tracing::warn!(
                txn_id = txn.id,
                pool = %pool_name,
                reason = "no_backend_selected",
                next_phase = ?continue_phase,
                "route failed: no backend selectable"
            );
            txn.set_rcode_name("SERVFAIL");
            txn.set_convergence_reason(ConvergenceReason::NoBackendSelected);
            return StageOutcome::Continue(continue_phase);
        };

        let backend_label = backend_metric_label_for_addr(&snapshot.config.pools, &pool, backend);
        txn.record_attempt(pool.clone(), backend, backend_label.clone());
        tracing::debug!(
            txn_id = txn.id,
            dns_id = txn.dns_id,
            pool = %pool,
            backend = %backend_label,
            backend_addr = %backend,
            attempt = txn.attempt_count,
            "route selected backend"
        );
        StageOutcome::Continue(Phase::Forward)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transaction::{ClientProtocol, Transaction};
    use conduit_config::load_yaml;
    use std::net::SocketAddr;

    fn minimal_snapshot() -> Arc<RuntimeSnapshot> {
        let yaml = include_str!("../../../../tests/fixtures/config/with-rhai-servfail-retry.yaml");
        let cfg = load_yaml(yaml).unwrap();
        let base = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures/config");
        Arc::new(
            RuntimeSnapshot::try_from_config_with_base(cfg, Some(&base)).expect("fixture snapshot"),
        )
    }

    #[test]
    fn first_route_ignores_stashed_retry_pool() {
        let snap = minimal_snapshot();
        let mut txn = Transaction::new(
            1,
            "127.0.0.1:53".parse::<SocketAddr>().unwrap(),
            ClientProtocol::Udp,
        );
        txn.selected_pool = Some("primary".into());
        txn.retry_pool = Some("secondary".into());
        RouteStage::new().handle(&mut txn, &snap);
        assert_eq!(txn.selected_pool.as_deref(), Some("primary"));
    }

    #[test]
    fn route_sets_backend_label_to_configured_name() {
        let yaml = r#"
schema_version: 1
listeners:
  listeners:
    - address: "127.0.0.1:15353"
      protocol: udp
pools:
  - name: primary
    backends:
      - address: "127.0.0.1:5300"
        name: resolver-east
        weight: 100
"#;
        let cfg = load_yaml(yaml).unwrap();
        let snap = Arc::new(RuntimeSnapshot::from_config(cfg));
        let mut txn = Transaction::new(
            1,
            "127.0.0.1:53".parse::<SocketAddr>().unwrap(),
            ClientProtocol::Udp,
        );
        txn.selected_pool = Some("primary".into());
        RouteStage::new().handle(&mut txn, &snap);
        assert_eq!(
            txn.selected_backend,
            Some("127.0.0.1:5300".parse::<SocketAddr>().unwrap())
        );
        assert_eq!(
            txn.selected_backend_display().as_deref(),
            Some("resolver-east")
        );
    }

    #[test]
    fn route_backend_label_falls_back_to_address_when_unnamed() {
        let yaml = r#"
schema_version: 1
listeners:
  listeners:
    - address: "127.0.0.1:15353"
      protocol: udp
pools:
  - name: primary
    backends:
      - address: "127.0.0.1:5300"
        weight: 100
"#;
        let cfg = load_yaml(yaml).unwrap();
        let snap = Arc::new(RuntimeSnapshot::from_config(cfg));
        let mut txn = Transaction::new(
            1,
            "127.0.0.1:53".parse::<SocketAddr>().unwrap(),
            ClientProtocol::Udp,
        );
        txn.selected_pool = Some("primary".into());
        RouteStage::new().handle(&mut txn, &snap);
        assert_eq!(
            txn.selected_backend_display().as_deref(),
            Some("127.0.0.1:5300")
        );
    }

    #[test]
    fn route_uses_default_pool_when_selected_pool_none() {
        let yaml = r#"
schema_version: 1
listeners:
  listeners:
    - address: "127.0.0.1:15353"
      protocol: udp
pools:
  - name: default
    backends:
      - address: "127.0.0.1:5300"
        weight: 100
  - name: vip
    backends:
      - address: "127.0.0.1:5301"
        weight: 100
"#;
        let cfg = load_yaml(yaml).unwrap();
        let snap = Arc::new(RuntimeSnapshot::from_config(cfg));
        let mut txn = Transaction::new(
            1,
            "127.0.0.1:53".parse::<SocketAddr>().unwrap(),
            ClientProtocol::Udp,
        );
        txn.selected_pool = None;
        RouteStage::new().handle(&mut txn, &snap);
        assert_eq!(txn.selected_pool.as_deref(), Some("default"));
        assert_eq!(
            txn.selected_backend,
            Some("127.0.0.1:5300".parse::<SocketAddr>().unwrap())
        );
    }

    #[test]
    fn retry_route_uses_default_when_selected_pool_cleared() {
        let yaml = r#"
schema_version: 1
listeners:
  listeners:
    - address: "127.0.0.1:15353"
      protocol: udp
pools:
  - name: default
    backends:
      - address: "127.0.0.1:5300"
        weight: 100
  - name: vip
    backends:
      - address: "127.0.0.1:5301"
        weight: 100
"#;
        let cfg = load_yaml(yaml).unwrap();
        let snap = Arc::new(RuntimeSnapshot::from_config(cfg));
        let mut txn = Transaction::new(
            1,
            "127.0.0.1:53".parse::<SocketAddr>().unwrap(),
            ClientProtocol::Udp,
        );
        txn.selected_pool = Some("vip".into());
        txn.attempt_count = 1;
        txn.selected_pool = None;
        RouteStage::new().handle(&mut txn, &snap);
        assert_eq!(txn.selected_pool.as_deref(), Some("default"));
    }

    #[test]
    fn retry_route_uses_stashed_retry_pool() {
        let snap = minimal_snapshot();
        let mut txn = Transaction::new(
            1,
            "127.0.0.1:53".parse::<SocketAddr>().unwrap(),
            ClientProtocol::Udp,
        );
        txn.selected_pool = Some("primary".into());
        txn.retry_pool = Some("secondary".into());
        txn.attempt_count = 1;
        RouteStage::new().handle(&mut txn, &snap);
        assert_eq!(txn.selected_pool.as_deref(), Some("secondary"));
        assert!(txn.retry_pool.is_none());
    }
}
