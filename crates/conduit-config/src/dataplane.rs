//! Dataplane runtime configuration helpers.

use crate::error::ConfigError;
use conduit_proto::config::Config;

pub const DEFAULT_DATAPLANE_RUNTIME: &str = "sync";
pub const DEFAULT_POLICY_WORKERS: u32 = 1;
pub const DEFAULT_IO_WORKERS: u32 = 1;

/// Effective dataplane runtime when the block is omitted.
pub fn effective_dataplane_runtime(cfg: &Config) -> &str {
    cfg.dataplane
        .as_ref()
        .map(|d| d.runtime.as_str())
        .filter(|r| !r.is_empty())
        .unwrap_or(DEFAULT_DATAPLANE_RUNTIME)
}

pub fn effective_policy_workers(cfg: &Config) -> u32 {
    cfg.dataplane
        .as_ref()
        .map(|d| d.policy_workers)
        .filter(|&n| n > 0)
        .unwrap_or(DEFAULT_POLICY_WORKERS)
}

pub fn effective_io_workers(cfg: &Config) -> u32 {
    cfg.dataplane
        .as_ref()
        .map(|d| d.io_workers)
        .filter(|&n| n > 0)
        .unwrap_or(DEFAULT_IO_WORKERS)
}

pub fn parse_dataplane_runtime(runtime: &str) -> Result<(), ConfigError> {
    match runtime {
        "sync" | "split_io" | "tokio" => Ok(()),
        _ => Err(ConfigError::Invalid(format!(
            "dataplane.runtime '{runtime}' must be sync, split_io, or tokio"
        ))),
    }
}

pub fn validate_dataplane(cfg: &Config) -> Vec<String> {
    let mut errors = Vec::new();
    let Some(dp) = &cfg.dataplane else {
        return errors;
    };

    let runtime = if dp.runtime.is_empty() {
        DEFAULT_DATAPLANE_RUNTIME
    } else {
        dp.runtime.as_str()
    };
    if let Err(e) = parse_dataplane_runtime(runtime) {
        errors.push(e.to_string());
    }

    if runtime == "split_io" || runtime == "tokio" {
        if dp.policy_workers == 0 {
            errors.push(
                "dataplane.policy_workers must be >= 1 when runtime is split_io or tokio".into(),
            );
        }
        if dp.io_workers == 0 {
            errors
                .push("dataplane.io_workers must be >= 1 when runtime is split_io or tokio".into());
        }
    }

    if let Some(chunk) = dp.slot_chunk_size {
        if chunk == 0 {
            errors.push("dataplane.slot_chunk_size must be >= 1 when set".into());
        }
    }

    errors
}
