//! Built-in and user metrics export, pipeline tracing store (phase 4).

mod build_metadata;
mod builtin;
mod compile;
mod export;
mod labels;
mod otel;
mod prometheus_http;
mod task;
mod trace;
mod user;

pub use build_metadata::{
    label_pairs as build_info_label_pairs, BUILD_PROFILE, DIRTY, REVISION, VERSION,
};
pub use builtin::{BuiltinRegistry, ScrapeGaugeSnapshot, ScrapeSnapshotFn};
pub use compile::{
    compile_from_config, trace_activation_matches, validate_metrics_tracing, BuiltinProfile,
    CompiledMetrics, CompiledTraceActivation, CompiledTracing,
};
pub use export::render_prometheus;
pub use labels::{ip_family_label, qclass_label, qtype_label, rcode_class_label, rcode_label};
pub use otel::{push_metrics_once, spawn_otel_push, OtelPushSettings};
pub use prometheus_http::spawn_prometheus_server;
pub use task::{OtelPushHandle, PrometheusServerHandle};
pub use trace::{TraceEvent, TraceLog, TraceStore};
pub use user::{UserMetricDelta, UserRegistry};

/// `reason` label values for [`BuiltinRegistry::record_script_error`].
pub const SCRIPT_ERROR_LOOKUP_UNKNOWN_TABLE: &str = "lookup_unknown_table";
pub const SCRIPT_ERROR_PHASE_GUARD: &str = "phase_guard";
pub const SCRIPT_ERROR_TIMEOUT: &str = "timeout";
pub const SCRIPT_ERROR_OPERATION_LIMIT: &str = "operation_limit";
pub const SCRIPT_ERROR_EVAL: &str = "eval";

use std::sync::Arc;

/// Process-wide metrics state shared by dataplane and export sinks.
pub struct MetricsHub {
    pub builtin: Arc<BuiltinRegistry>,
    pub user: Arc<UserRegistry>,
    pub compiled: CompiledMetrics,
}

impl MetricsHub {
    pub fn set_scrape_snapshot_fn(&self, f: ScrapeSnapshotFn) {
        self.builtin.set_scrape_snapshot_fn(f);
    }

    pub fn from_config(config: &conduit_proto::config::Config) -> Self {
        let (compiled, _) = compile_from_config(config);
        let builtin = Arc::new(BuiltinRegistry::new(compiled.enabled, compiled.profile));
        let user = Arc::new(UserRegistry::new(compiled.enabled));
        Self {
            builtin,
            user,
            compiled,
        }
    }

    pub fn metrics_enabled(&self) -> bool {
        self.compiled.enabled
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
