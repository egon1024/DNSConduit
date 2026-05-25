//! Rhai scripting: compile at snapshot build, execute at rule hooks (design §6, §9).

mod compile;
mod data_sources;
mod error;
mod host;
mod metrics;
mod runtime;

pub use compile::{compile_from_config, CompiledScripting, ScriptRef};
pub use data_sources::DataSourceStore;
pub use error::ScriptError;
pub use host::{HostTransaction, ScriptPhase};
pub use metrics::{MetricRegistry, UserMetricDef};
pub use runtime::{rhai_script_errors_total, run_scripts, ScriptRunOutcome, ScriptRunStats};
