//! Rhai host API surface objects (`runtime`, `lookup`, `metrics`, `log`).

use crate::data_sources::DataSourceStore;
use crate::metrics::MetricRegistry;
use crate::routing_view::{BackendRoutingView, PoolRoutingView, RoutingRuntimeSnapshot};
use crate::runtime::{queue_user_metric, ScriptEffects};
use crate::script_errors::{
    report_lookup_unknown_table, report_script_log_info, report_script_log_warn,
};
use rhai::{CustomType, Dynamic, EvalAltResult, TypeBuilder};
use std::net::IpAddr;
use std::sync::{Arc, Mutex};

#[derive(Clone)]
pub struct LogView;

impl LogView {
    pub fn info(&self, message: &str) {
        crate::runtime::SCRIPT_RUN_CTX.with(|cell| {
            if let Some(ctx) = cell.borrow().as_ref() {
                report_script_log_info(
                    ctx.snapshot_generation,
                    &ctx.script_path,
                    &ctx.rule_name,
                    ctx.txn_id,
                    message,
                );
            }
        });
    }

    pub fn warn(&self, message: &str) {
        crate::runtime::SCRIPT_RUN_CTX.with(|cell| {
            if let Some(ctx) = cell.borrow().as_ref() {
                report_script_log_warn(
                    ctx.snapshot_generation,
                    &ctx.script_path,
                    &ctx.rule_name,
                    ctx.txn_id,
                    message,
                );
            }
        });
    }
}

#[derive(Clone)]
pub struct LookupView {
    store: Arc<DataSourceStore>,
}

impl LookupView {
    pub fn new(store: Arc<DataSourceStore>) -> Self {
        Self { store }
    }

    pub fn lookup(&self, table: &str, key: &str) -> String {
        if !self.store.has_table(table) {
            crate::runtime::SCRIPT_RUN_CTX.with(|cell| {
                if let Some(ctx) = cell.borrow().as_ref() {
                    report_lookup_unknown_table(
                        ctx.builtin.as_deref(),
                        ctx.snapshot_generation,
                        &ctx.script_path,
                        &ctx.rule_name,
                        table,
                    );
                }
            });
            return String::new();
        }
        self.store.lookup(table, key)
    }

    /// Longest-prefix lookup over a named `type: cidr` data source. Returns
    /// `""` for a miss (unknown table, unparseable `addr`, or no matching
    /// prefix); a hit is always non-empty (see `load_cidr_table`).
    pub fn lookup_ip(&self, name: &str, addr: &str) -> String {
        if !self.store.has_cidr_table(name) {
            crate::runtime::SCRIPT_RUN_CTX.with(|cell| {
                if let Some(ctx) = cell.borrow().as_ref() {
                    report_lookup_unknown_table(
                        ctx.builtin.as_deref(),
                        ctx.snapshot_generation,
                        &ctx.script_path,
                        &ctx.rule_name,
                        name,
                    );
                }
            });
            return String::new();
        }
        let Ok(ip) = addr.parse::<IpAddr>() else {
            tracing::warn!(table = %name, addr = %addr, "lookup_ip: invalid IP address argument");
            return String::new();
        };
        self.store.lookup_ip(name, ip).unwrap_or_default().into()
    }
}

#[derive(Clone)]
pub struct MetricsView {
    registry: Arc<MetricRegistry>,
    effects: Arc<Mutex<ScriptEffects>>,
}

impl MetricsView {
    pub fn new(registry: Arc<MetricRegistry>, effects: Arc<Mutex<ScriptEffects>>) -> Self {
        Self { registry, effects }
    }

    pub fn inc(&self, name: &str, delta: i64) -> Result<(), Box<EvalAltResult>> {
        queue_user_metric(&self.registry, &self.effects, name, delta, &[])
    }

    pub fn inc_labels(
        &self,
        name: &str,
        delta: i64,
        labels: rhai::Map,
    ) -> Result<(), Box<EvalAltResult>> {
        let pairs = rhai_map_to_pairs(labels)?;
        queue_user_metric(&self.registry, &self.effects, name, delta, &pairs)
    }
}

#[derive(Clone)]
pub struct RuntimeView {
    routing: RoutingRuntimeView,
    config_generation: u64,
}

impl RuntimeView {
    pub fn new(snapshot: Arc<RoutingRuntimeSnapshot>) -> Self {
        let config_generation = snapshot.config_generation();
        Self {
            routing: RoutingRuntimeView { snapshot },
            config_generation,
        }
    }

    pub fn routing(&self) -> RoutingRuntimeView {
        self.routing.clone()
    }

    pub fn config_generation(&self) -> i64 {
        self.config_generation as i64
    }
}

#[derive(Clone)]
pub struct RoutingRuntimeView {
    snapshot: Arc<RoutingRuntimeSnapshot>,
}

impl RoutingRuntimeView {
    pub fn pool(&self, name: &str) -> RhaiPoolView {
        RhaiPoolView(self.snapshot.pool(name))
    }

    pub fn backend(&self, pool: &str, id: &str) -> RhaiBackendView {
        RhaiBackendView(self.snapshot.backend(pool, id))
    }

    pub fn backend_for_attempt(&self, pool: String, backend_id: String) -> RhaiBackendView {
        if pool.is_empty() || backend_id.is_empty() {
            return RhaiBackendView(BackendRoutingView::EMPTY);
        }
        RhaiBackendView(self.snapshot.backend(&pool, &backend_id))
    }
}

#[derive(Clone)]
pub struct RhaiPoolView(PoolRoutingView);

impl RhaiPoolView {
    pub fn configured(&self) -> bool {
        self.0.configured
    }

    pub fn configured_count(&self) -> i64 {
        self.0.configured_count as i64
    }

    pub fn eligible_count(&self) -> i64 {
        self.0.eligible_count as i64
    }

    pub fn fail_open_active(&self) -> bool {
        self.0.fail_open_active
    }

    pub fn max_outstanding(&self) -> i64 {
        self.0.max_outstanding as i64
    }

    pub fn min_latency_ewma_ms(&self) -> Dynamic {
        match self.0.min_latency_ewma_ms {
            Some(v) => Dynamic::from(v),
            None => Dynamic::UNIT,
        }
    }
}

#[derive(Clone)]
pub struct RhaiBackendView(BackendRoutingView);

impl RhaiBackendView {
    pub fn configured(&self) -> bool {
        self.0.configured
    }

    pub fn applied(&self) -> &str {
        self.0.applied
    }

    pub fn observed(&self) -> &str {
        self.0.observed
    }

    pub fn eligible(&self) -> bool {
        self.0.eligible
    }

    pub fn frozen(&self) -> bool {
        self.0.frozen
    }

    pub fn weight_factor(&self) -> f64 {
        self.0.weight_factor
    }

    pub fn outstanding(&self) -> i64 {
        self.0.outstanding as i64
    }

    pub fn latency_ewma_ms(&self) -> Dynamic {
        match self.0.latency_ewma_ms {
            Some(v) => Dynamic::from(v),
            None => Dynamic::UNIT,
        }
    }

    pub fn last_transition_unix_ms(&self) -> Dynamic {
        match self.0.last_transition_unix_ms {
            Some(v) => Dynamic::from(v as i64),
            None => Dynamic::UNIT,
        }
    }
}

fn rhai_map_to_pairs(map: rhai::Map) -> Result<Vec<(String, String)>, Box<EvalAltResult>> {
    let mut pairs = Vec::with_capacity(map.len());
    for (k, v) in map {
        let key = k.to_string();
        let value = v.to_string();
        pairs.push((key, value));
    }
    Ok(pairs)
}

pub fn register_host_surfaces(engine: &mut rhai::Engine) {
    engine.build_type::<LogView>();
    engine.build_type::<LookupView>();
    engine.build_type::<MetricsView>();
    engine.build_type::<RuntimeView>();
    engine.build_type::<RoutingRuntimeView>();
    engine.build_type::<RhaiPoolView>();
    engine.build_type::<RhaiBackendView>();
}

impl CustomType for LogView {
    fn build(mut builder: TypeBuilder<Self>) {
        builder
            .with_name("Log")
            .with_fn("info", |log: LogView, message: String| {
                log.info(&message);
            })
            .with_fn("warn", |log: LogView, message: String| {
                log.warn(&message);
            });
    }
}

impl CustomType for LookupView {
    fn build(mut builder: TypeBuilder<Self>) {
        builder
            .with_name("Lookup")
            .with_fn(
                "lookup",
                |view: LookupView, table: String, key: String| -> String {
                    view.lookup(&table, &key)
                },
            )
            .with_fn(
                "lookup_ip",
                |view: LookupView, name: String, addr: String| -> String {
                    view.lookup_ip(&name, &addr)
                },
            );
    }
}

impl CustomType for MetricsView {
    fn build(mut builder: TypeBuilder<Self>) {
        builder
            .with_name("Metrics")
            .with_fn(
                "inc",
                |metrics: MetricsView,
                 name: String,
                 delta: i64|
                 -> Result<(), Box<EvalAltResult>> { metrics.inc(&name, delta) },
            )
            .with_fn(
                "inc_labels",
                |metrics: MetricsView,
                 name: String,
                 delta: i64,
                 labels: rhai::Map|
                 -> Result<(), Box<EvalAltResult>> {
                    metrics.inc_labels(&name, delta, labels)
                },
            );
    }
}

impl CustomType for RuntimeView {
    fn build(mut builder: TypeBuilder<Self>) {
        builder
            .with_name("Runtime")
            .with_fn("routing", |runtime: RuntimeView| -> RoutingRuntimeView {
                runtime.routing()
            })
            .with_fn("config_generation", |runtime: RuntimeView| -> i64 {
                runtime.config_generation()
            });
    }
}

impl CustomType for RoutingRuntimeView {
    fn build(mut builder: TypeBuilder<Self>) {
        builder
            .with_name("RoutingRuntime")
            .with_fn(
                "pool",
                |routing: RoutingRuntimeView, name: String| -> RhaiPoolView { routing.pool(&name) },
            )
            .with_fn(
                "backend",
                |routing: RoutingRuntimeView, pool: String, id: String| -> RhaiBackendView {
                    routing.backend(&pool, &id)
                },
            )
            .with_fn(
                "backend_for_attempt",
                |routing: RoutingRuntimeView,
                 pool: String,
                 backend_id: String|
                 -> RhaiBackendView {
                    routing.backend_for_attempt(pool, backend_id)
                },
            );
    }
}

impl CustomType for RhaiPoolView {
    fn build(mut builder: TypeBuilder<Self>) {
        builder
            .with_name("PoolRuntime")
            .with_fn("configured", |view: RhaiPoolView| -> bool {
                view.configured()
            })
            .with_fn("configured_count", |view: RhaiPoolView| -> i64 {
                view.configured_count()
            })
            .with_fn("eligible_count", |view: RhaiPoolView| -> i64 {
                view.eligible_count()
            })
            .with_fn("fail_open_active", |view: RhaiPoolView| -> bool {
                view.fail_open_active()
            })
            .with_fn("max_outstanding", |view: RhaiPoolView| -> i64 {
                view.max_outstanding()
            })
            .with_fn("min_latency_ewma_ms", |view: RhaiPoolView| -> Dynamic {
                view.min_latency_ewma_ms()
            });
    }
}

impl CustomType for RhaiBackendView {
    fn build(mut builder: TypeBuilder<Self>) {
        builder
            .with_name("BackendRuntime")
            .with_fn("configured", |view: RhaiBackendView| -> bool {
                view.configured()
            })
            .with_fn("applied", |view: RhaiBackendView| -> String {
                view.applied().to_string()
            })
            .with_fn("observed", |view: RhaiBackendView| -> String {
                view.observed().to_string()
            })
            .with_fn("eligible", |view: RhaiBackendView| -> bool {
                view.eligible()
            })
            .with_fn("frozen", |view: RhaiBackendView| -> bool { view.frozen() })
            .with_fn("weight_factor", |view: RhaiBackendView| -> f64 {
                view.weight_factor()
            })
            .with_fn("outstanding", |view: RhaiBackendView| -> i64 {
                view.outstanding()
            })
            .with_fn("latency_ewma_ms", |view: RhaiBackendView| -> Dynamic {
                view.latency_ewma_ms()
            })
            .with_fn(
                "last_transition_unix_ms",
                |view: RhaiBackendView| -> Dynamic { view.last_transition_unix_ms() },
            );
    }
}
