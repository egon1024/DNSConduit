//! Built-in and user metrics export, pipeline tracing store (phase 4).
//!
//! Hot-swap support (metrics-configurability G4): the [`MetricsHub`] holds a
//! process-lifetime [`MetricStore`] plus hot-updatable components via
//! [`arc_swap::ArcSwap`]. Call [`MetricsHub::apply_compiled`] after a
//! successful config reload to swap in the new plan. In-flight transactions
//! pin their metrics registries via [`MetricsPin`] to ensure continuity.
//!
//! Hot-rebind support (metrics-configurability G4, tasks 5.9–5.10): the
//! [`MetricsExportController`] manages Prometheus and OTLP sinks with a
//! prepare/commit pattern. Changing `listen_address`/`path` (Prom) or
//! `endpoint`/TLS (OTLP) pre-binds/reconnects before snapshot install;
//! bind failure rejects apply and keeps the last-good sink. Plan-only
//! changes (categories, collect/emit, granularity) skip rebind entirely.

mod build_metadata;
mod builtin;
mod compile;
mod consumer;
mod export;
mod export_controller;
mod labels;
mod otel;
mod plan;
mod prometheus_http;
mod store;
mod task;
mod trace;
mod user;

pub use build_metadata::{
    label_pairs as build_info_label_pairs, BUILD_PROFILE, DIRTY, REVISION, VERSION,
};
pub use builtin::{
    encode_builtin, BackendIdentity, BuiltinRegistry, CacheCapacitySample, HealthScrapeBackend,
    ListenerIdentity, ScrapeGaugeSnapshot, ScrapeSnapshotFn,
};
pub use compile::{
    compile_from_config, trace_activation_matches, validate_metrics_tracing, BuiltinProfile,
    CompiledMetrics, CompiledTraceActivation, CompiledTracing,
};
pub use consumer::{
    check_consumer_dependencies, conduit_user_metric_name, is_write_consumer_symbol,
    ConsumerDependencyReport, ConsumerKind, MetricConsumerGraph, MetricConsumerRef,
    SidecarConsumerRegistry, WasmConsumerRegistry,
};
pub use export::{gather_prometheus_families, render_prometheus};
pub use export_controller::{
    MetricsExportController, OtelChange, PendingExportChange, PrometheusChange,
};
pub use labels::{ip_family_label, qclass_label, qtype_label, rcode_class_label, rcode_label};
pub use otel::{build_otel_push_loop, push_metrics_once, spawn_otel_push, OtelPushSettings};
pub use plan::{
    builtin_metric_category, default_granularity_for_base, default_responses_rcode, expand_base,
    family_allowed_dimensions, preset_family_dimensions, resolve_metrics_plan, CompiledMetricsPlan,
    Granularity, MetricCategory, MetricsBase, PlanResolution, ResponsesRcodeBucketing,
    UserMetricMode, PROFILE_DEPRECATION_WARNING, USER_METRIC_EXPORT_DEPRECATION_WARNING,
};
pub use prometheus_http::{spawn_prometheus_server, PrometheusServer};
pub use store::{MetricStore, SeriesIdentity};
pub use task::{OtelPushHandle, PrometheusServerHandle};
pub use trace::{TraceEvent, TraceLog, TraceStore};
pub use user::{UserMetricDelta, UserRegistry, DEFAULT_USER_METRIC_HELP};

/// `reason` label values for [`BuiltinRegistry::record_script_error`].
pub const SCRIPT_ERROR_LOOKUP_UNKNOWN_TABLE: &str = "lookup_unknown_table";
pub const SCRIPT_ERROR_PHASE_GUARD: &str = "phase_guard";
pub const SCRIPT_ERROR_TIMEOUT: &str = "timeout";
pub const SCRIPT_ERROR_OPERATION_LIMIT: &str = "operation_limit";
pub const SCRIPT_ERROR_EVAL: &str = "eval";

use arc_swap::{ArcSwap, Guard};
use parking_lot::RwLock;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

/// Hot-swappable metrics components held under a single `ArcSwap`.
///
/// This struct groups everything that changes on plan swap so a single atomic
/// load gets a consistent view. The `MetricStore` is *not* included here
/// because it has process lifetime and is never swapped.
pub struct HotMetrics {
    /// The builtin registry for this plan generation.
    pub builtin: Arc<BuiltinRegistry>,
    /// The user-defined metric registry for this plan generation.
    pub user: Arc<UserRegistry>,
    /// The compiled metrics configuration (emit/collect masks, endpoints, etc.).
    pub compiled: CompiledMetrics,
}

/// Process-wide metrics state shared by dataplane and export sinks.
///
/// Ownership model (for Rust newcomers):
/// - `store`: `Arc<MetricStore>` lives for the entire process. Metric handles
///   obtained from it survive plan swaps, preserving counter/histogram values.
/// - `hot`: `ArcSwap<HotMetrics>` holds the current plan's registries. Swapping
///   is lock-free; readers get a `Guard` that keeps the old value alive while
///   they use it (like a read-lock but wait-free).
/// - `scrape_fn`: stored separately so we can re-apply it after a swap.
/// - `generation`: incremented on every `apply_compiled`; used by transaction
///   pins to identify which plan they hold.
pub struct MetricsHub {
    /// Process-lifetime handle store (counter/histogram/gauge handles survive
    /// across plan swaps). Handles with matching series identity are reused.
    store: Arc<MetricStore>,
    /// Hot-swappable components (builtin registry, user registry, compiled plan).
    hot: ArcSwap<HotMetrics>,
    /// Scrape snapshot function; stored on the hub so we can re-apply it when
    /// the builtin registry is swapped.
    scrape_fn: RwLock<Option<ScrapeSnapshotFn>>,
    /// Monotonically increasing generation; bumped on each `apply_compiled`.
    generation: AtomicU64,
}

impl MetricsHub {
    /// Accessor for the current builtin registry `Arc`. The Arc is cloned
    /// (cheap: just bumps refcount) so callers get a consistent view even
    /// if a swap happens mid-use.
    ///
    /// This is the primary accessor for recording metrics. Example:
    /// ```ignore
    /// hub.builtin().record_query(...);
    /// ```
    pub fn builtin(&self) -> Arc<BuiltinRegistry> {
        Arc::clone(&self.hot.load().builtin)
    }

    /// Accessor for the current user registry `Arc`.
    pub fn user(&self) -> Arc<UserRegistry> {
        Arc::clone(&self.hot.load().user)
    }

    /// Accessor for the current compiled metrics config.
    pub fn compiled(&self) -> Guard<Arc<HotMetrics>> {
        self.hot.load()
    }

    /// Current metrics generation (bumped on each `apply_compiled`).
    pub fn generation(&self) -> u64 {
        self.generation.load(Ordering::Acquire)
    }

    /// Process-lifetime metric store (for handle reuse across swaps).
    pub fn store(&self) -> &Arc<MetricStore> {
        &self.store
    }

    pub fn set_scrape_snapshot_fn(&self, f: ScrapeSnapshotFn) {
        // Store the function so we can re-apply it after swaps.
        *self.scrape_fn.write() = Some(f.clone());
        // Also set it on the current builtin registry.
        self.hot.load().builtin.set_scrape_snapshot_fn(f);
    }

    pub fn from_config(config: &conduit_proto::config::Config) -> Self {
        let (compiled, _) = compile_from_config(config);
        let store = Arc::new(MetricStore::new());
        // Plan-driven registration (metrics-configurability): `health` and the
        // volume/failures/timing collect mask follow `compiled.plan` rather
        // than the legacy `profile` two-tier split. See
        // `BuiltinRegistry::new_from_plan_with_store`.
        let builtin = Arc::new(BuiltinRegistry::new_from_plan_with_store(
            &compiled.plan,
            &store,
        ));
        let user = Arc::new(UserRegistry::new_with_helps(
            compiled.enabled,
            compiled.plan.user_helps(),
        ));
        let hot = HotMetrics {
            builtin,
            user,
            compiled,
        };
        Self {
            store,
            hot: ArcSwap::from_pointee(hot),
            scrape_fn: RwLock::new(None),
            generation: AtomicU64::new(1),
        }
    }

    /// Hot-swap the metrics plan after a successful config reload.
    ///
    /// Builds new registries from the same `MetricStore` (preserving counter
    /// values for overlapping series identities), swaps them atomically, and
    /// re-applies the scrape snapshot function.
    ///
    /// Call this from the configurator after `install_validated_with_base`
    /// succeeds. The new `compiled` should come from the newly installed
    /// snapshot's metrics config.
    pub fn apply_compiled(&self, compiled: CompiledMetrics) {
        // Build new registries using the same store so handles are reused.
        let builtin = Arc::new(BuiltinRegistry::new_from_plan_with_store(
            &compiled.plan,
            &self.store,
        ));
        let user = Arc::new(UserRegistry::new_with_helps(
            compiled.enabled,
            compiled.plan.user_helps(),
        ));

        // Re-apply scrape snapshot function to the new builtin registry.
        if let Some(f) = self.scrape_fn.read().clone() {
            builtin.set_scrape_snapshot_fn(f);
        }

        let hot = HotMetrics {
            builtin,
            user,
            compiled,
        };
        self.hot.store(Arc::new(hot));
        self.generation.fetch_add(1, Ordering::AcqRel);
        tracing::debug!(generation = self.generation(), "metrics plan swapped");
    }

    pub fn metrics_enabled(&self) -> bool {
        self.hot.load().compiled.enabled
    }

    /// Acquire a pin for the current metrics registries. The returned
    /// [`MetricsPin`] holds `Arc` clones that keep the registries alive
    /// even after a subsequent `apply_compiled` swap.
    ///
    /// Transactions should acquire a pin at orchestrator start and use it
    /// for all metric recording during their lifetime. This ensures
    /// consistent series identity across a single query's lifecycle.
    pub fn acquire_pin(&self) -> MetricsPin {
        let hot = self.hot.load();
        MetricsPin {
            builtin: Arc::clone(&hot.builtin),
            generation: self.generation.load(Ordering::Acquire),
        }
    }
}

/// Pin holding references to metrics registries at a specific generation.
///
/// Acquired at transaction start via [`MetricsHub::acquire_pin`]. Keeps the
/// registries alive for the transaction's lifetime even if the hub swaps to
/// a new plan. This ensures counter increments within a single query go to
/// consistent handles.
///
/// Ownership model: cloning the inner `Arc`s bumps their reference counts.
/// When the last pin for a generation drops, the old registries become
/// eligible for drop (and their Prometheus `Registry` objects are freed).
/// The underlying metric *handles* in `MetricStore` survive because the
/// store has process lifetime.
#[derive(Clone)]
pub struct MetricsPin {
    /// Pinned builtin registry (may be older than current hub generation).
    pub builtin: Arc<BuiltinRegistry>,
    /// Generation at which this pin was acquired.
    pub generation: u64,
}

// Provide direct access to commonly used fields via Deref-like accessors
// on the Guard. This keeps call sites concise.
impl std::ops::Deref for HotMetrics {
    type Target = CompiledMetrics;
    fn deref(&self) -> &Self::Target {
        &self.compiled
    }
}

/// Tracing config + in-memory store for GetTrace.
pub struct TracingHub {
    pub compiled: CompiledTracing,
    pub store: Arc<TraceStore>,
}

impl TracingHub {
    pub fn from_config(config: &conduit_proto::config::Config) -> Self {
        let (_, compiled) = compile_from_config(config);
        Self {
            compiled,
            store: Arc::new(TraceStore::new(1000, std::time::Duration::from_secs(300))),
        }
    }

    pub fn tracing_enabled(&self) -> bool {
        self.compiled.enabled
    }
}
