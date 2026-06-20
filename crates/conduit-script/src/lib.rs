//! Rhai scripting: compile at snapshot build, execute at rule hooks (design §6, §9).

mod compile;
mod data_sources;
mod error;
mod host;
mod lookup_scan;
mod metrics;
mod runtime;
mod script_errors;

#[cfg(any(test, feature = "test-util"))]
pub mod testing;

pub use compile::{compile_from_config, CompiledScripting, ScriptRef};
pub use data_sources::DataSourceStore;
pub use error::ScriptError;
pub use host::{HostTransaction, ScriptPhase};
pub use metrics::{MetricRegistry, UserMetricDef, UserMetricExportTier};
pub use runtime::{run_scripts, ScriptRunOutcome, ScriptRunStats};
pub use script_errors::rhai_script_errors_total;
