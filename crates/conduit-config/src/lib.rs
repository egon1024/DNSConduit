//! Configuration load, validate, merge, and export.

pub mod error;
pub mod file;
pub mod merge;
pub mod validate;

pub use error::ConfigError;
pub use file::load_yaml;
pub use merge::{clear_overlay, merge_file_and_overlay, EffectiveConfig};
pub use validate::{validate, ValidationResult};
