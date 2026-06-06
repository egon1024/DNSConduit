//! Initialize `tracing` subscriber from config (phase 1 basic logging).

use crate::error::ConfigError;
use conduit_proto::config::LoggingConfig;
use std::io;
use tracing_subscriber::fmt::writer::BoxMakeWriter;
use tracing_subscriber::EnvFilter;

/// Default when `logging` is omitted from config.
pub const DEFAULT_LOG_LEVEL: &str = "info";
/// stderr keeps stdout free and matches common daemon conventions; use `stdout` in config if preferred.
pub const DEFAULT_LOG_OUTPUT: &str = "stderr";

/// Strip ASCII C0 control characters from text destined for log fields or messages.
///
/// Tracing output must be plain text for operators, log shippers, and terminals.
/// DNS names and other dynamic values may contain bytes that decode to controls.
pub fn log_text(input: &str) -> String {
    if !input.chars().any(|c| c.is_control()) {
        return input.to_string();
    }
    input.chars().filter(|c| !c.is_control()).collect()
}

/// Build the filter string from config. `RUST_LOG` wins when set in the environment.
pub fn env_filter_from_config(cfg: Option<&LoggingConfig>) -> Result<EnvFilter, ConfigError> {
    let level = cfg
        .map(|l| l.level.as_str())
        .filter(|s| !s.is_empty())
        .unwrap_or(DEFAULT_LOG_LEVEL);
    validate_level(level)?;

    let fallback = EnvFilter::new(level);
    Ok(EnvFilter::try_from_default_env().unwrap_or(fallback))
}

/// Install global fmt subscriber. Call once at process start after config load.
pub fn init_from_config(cfg: Option<&LoggingConfig>) -> Result<(), ConfigError> {
    let filter = env_filter_from_config(cfg)?;
    let writer = log_writer(cfg)?;
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(writer)
        .with_target(true)
        .with_ansi(false)
        .try_init()
        .map_err(|e| ConfigError::Invalid(format!("logging init failed: {e}")))?;
    Ok(())
}

fn log_writer(cfg: Option<&LoggingConfig>) -> Result<BoxMakeWriter, ConfigError> {
    let output = cfg
        .map(|l| l.output.as_str())
        .filter(|s| !s.is_empty())
        .unwrap_or(DEFAULT_LOG_OUTPUT);
    match output {
        "stderr" => Ok(BoxMakeWriter::new(io::stderr)),
        "stdout" => Ok(BoxMakeWriter::new(io::stdout)),
        other => Err(ConfigError::Invalid(format!(
            "logging.output must be stderr or stdout, got '{other}'"
        ))),
    }
}

pub fn validate_level(level: &str) -> Result<(), ConfigError> {
    match level {
        "error" | "warn" | "info" | "debug" | "trace" => Ok(()),
        _ => Err(ConfigError::Invalid(format!(
            "logging.level must be error|warn|info|debug|trace, got '{level}'"
        ))),
    }
}

pub fn validate_logging(cfg: Option<&LoggingConfig>) -> Result<(), ConfigError> {
    let Some(cfg) = cfg else {
        return Ok(());
    };
    if !cfg.level.is_empty() {
        validate_level(&cfg.level)?;
    }
    if !cfg.output.is_empty() && cfg.output != "stderr" && cfg.output != "stdout" {
        return Err(ConfigError::Invalid(format!(
            "logging.output must be stderr or stdout, got '{}'",
            cfg.output
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_filter_uses_info_without_rust_log() {
        std::env::remove_var("RUST_LOG");
        let filter = env_filter_from_config(None).unwrap();
        assert_eq!(filter.to_string(), "info");
    }

    #[test]
    fn rejects_invalid_level() {
        let cfg = LoggingConfig {
            level: "verbose".into(),
            output: String::new(),
        };
        assert!(validate_logging(Some(&cfg)).is_err());
    }

    #[test]
    fn accepts_stdout_output() {
        let cfg = LoggingConfig {
            level: "debug".into(),
            output: "stdout".into(),
        };
        assert!(validate_logging(Some(&cfg)).is_ok());
    }

    #[test]
    fn log_text_strips_control_characters() {
        assert_eq!(log_text("example.com"), "example.com");
        assert_eq!(log_text("a\u{1b}b"), "ab");
        assert_eq!(log_text("no\u{7f}ise"), "noise");
    }
}
