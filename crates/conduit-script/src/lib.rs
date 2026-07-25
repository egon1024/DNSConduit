//! Rhai scripting: compile at snapshot build, execute at rule hooks (design §6, §9).

mod capability_scan;
mod cidr;
mod compile;
mod data_sources;
mod dns_wire;
mod error;
mod host;
mod host_api;
mod lookup_scan;
mod metrics;
mod routing_view;
mod runtime;
mod script_errors;

#[cfg(any(test, feature = "test-util"))]
pub mod testing;

pub use compile::{compile_from_config, CompiledScripting, ScriptRef};
pub use data_sources::DataSourceStore;
pub use dns_wire::{
    qclass_canonical_name, qtype_canonical_name, rcode_canonical_name, DnsOpcode, EdnsOptionCode,
    QueryClass, Rcode, RecordType,
};
pub use error::ScriptError;
pub use host::{
    unix_secs, utc_hour_and_weekday, ClientProtocol, HostTransaction, ResponseWireMeta, ScriptPhase,
};
pub use metrics::{
    scan_metric_sites, scan_metrics_from_source, MetricRegistry, MetricScanSite, UserMetricDef,
    UserMetricExportTier,
};
pub use routing_view::{BackendRoutingView, PoolRoutingView, RoutingRuntimeSnapshot};
pub use runtime::{run_scripts, ScriptRunOutcome, ScriptRunStats};
pub use script_errors::rhai_script_errors_total;
