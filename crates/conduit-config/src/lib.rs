//! Configuration load, validate, merge, and export.

pub mod backend;
pub mod error;
pub mod export;
pub mod file;
pub mod logging;
pub mod merge;
pub mod validate;

pub use backend::{effective_backend_weight, DEFAULT_BACKEND_WEIGHT};
pub use error::ConfigError;
pub use export::export_yaml;
pub use file::load_yaml;
pub use logging::{init_from_config, validate_logging, DEFAULT_LOG_LEVEL, DEFAULT_LOG_OUTPUT};
pub use merge::{clear_overlay, merge_file_and_overlay, EffectiveConfig};
pub use validate::{validate, ValidationResult};
