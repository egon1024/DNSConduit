//! Configuration load, validate, merge, and export.

pub mod error;
pub mod file;
pub mod validate;

pub use error::ConfigError;
pub use file::load_yaml;
pub use validate::{validate, ValidationResult};
