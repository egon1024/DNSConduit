//! File baseline + API overlay merge (spec §5.2).
//!
//! # Phase 0 merge strategy
//!
//! - **Top-level sections** (`listeners`, `forward`, `orchestrator`, `events`,
//!   `rhai`, `control`, `dataplane`, `shutdown`): if the overlay has the section set (`Option::is_some`), replace
//!   the entire sub-message on the effective config.
//! - **`schema_version`**: overlay value wins.
//! - **`pools`**: match pools by `name`; for each overlay pool, match backends by
//!   `name` when the overlay entry has a non-empty `name`, else by `address`.
//!   Unknown overlay backend `name` is rejected. Overlay backends matched only by
//!   `address` that are not present in a pool are appended.

use crate::error::ConfigError;
use conduit_proto::config::{Backend, Config, Pool};

#[derive(Debug, Clone)]
pub struct EffectiveConfig {
    pub file: Config,
    pub overlay: Option<Config>,
}

impl EffectiveConfig {
    pub fn new(file: Config) -> Self {
        Self {
            file,
            overlay: None,
        }
    }

    /// Effective configuration after applying overlay, if any.
    pub fn effective(&self) -> Config {
        match &self.overlay {
            Some(overlay) => merge_file_and_overlay(&self.file, overlay)
                .expect("overlay validated at apply time"),
            None => self.file.clone(),
        }
    }
}

pub fn merge_file_and_overlay(file: &Config, overlay: &Config) -> Result<Config, ConfigError> {
    let mut merged = file.clone();

    merged.schema_version = overlay.schema_version;

    if overlay.listeners.is_some() {
        merged.listeners = overlay.listeners.clone();
    }
    if overlay.forward.is_some() {
        merged.forward = overlay.forward.clone();
    }
    if overlay.orchestrator.is_some() {
        merged.orchestrator = overlay.orchestrator;
    }
    if overlay.events.is_some() {
        merged.events = overlay.events.clone();
    }
    if overlay.rhai.is_some() {
        merged.rhai = overlay.rhai;
    }
    if overlay.control.is_some() {
        merged.control = overlay.control.clone();
    }
    if overlay.logging.is_some() {
        merged.logging = overlay.logging.clone();
    }
    if overlay.dataplane.is_some() {
        merged.dataplane = overlay.dataplane.clone();
    }
    if overlay.shutdown.is_some() {
        merged.shutdown = overlay.shutdown;
    }
    if !overlay.data_sources.is_empty() {
        merged.data_sources = overlay.data_sources.clone();
    }

    if !overlay.pools.is_empty() {
        merge_pools(&mut merged.pools, &overlay.pools)?;
    }

    Ok(merged)
}

pub fn clear_overlay(effective: &mut EffectiveConfig) {
    effective.overlay = None;
}

/// True when the patch sets no overlay-eligible fields (`schema_version` alone does not count).
pub fn is_overlay_patch_empty(cfg: &Config) -> bool {
    cfg.listeners.is_none()
        && cfg.forward.is_none()
        && cfg.orchestrator.is_none()
        && cfg.events.is_none()
        && cfg.rhai.is_none()
        && cfg.control.is_none()
        && cfg.logging.is_none()
        && cfg.dataplane.is_none()
        && cfg.shutdown.is_none()
        && cfg.pools.is_empty()
        && cfg.data_sources.is_empty()
}

/// Merge one overlay patch into another (same rules as [`merge_file_and_overlay`]).
pub fn merge_overlay_patches(base: &Config, patch: &Config) -> Result<Config, ConfigError> {
    merge_file_and_overlay(base, patch)
}

fn merge_pools(base: &mut Vec<Pool>, overlay: &[Pool]) -> Result<(), ConfigError> {
    for overlay_pool in overlay {
        if let Some(base_pool) = base.iter_mut().find(|p| p.name == overlay_pool.name) {
            merge_backends(
                &base_pool.name,
                &mut base_pool.backends,
                &overlay_pool.backends,
            )?;
        } else {
            base.push(overlay_pool.clone());
        }
    }
    Ok(())
}

fn merge_backends(
    pool_name: &str,
    base: &mut Vec<Backend>,
    overlay: &[Backend],
) -> Result<(), ConfigError> {
    for overlay_backend in overlay {
        if let Some(name) = overlay_backend.name.as_ref().filter(|n| !n.is_empty()) {
            if let Some(base_backend) = base
                .iter_mut()
                .find(|b| b.name.as_deref() == Some(name.as_str()))
            {
                apply_backend_overlay(base_backend, overlay_backend);
            } else {
                return Err(ConfigError::Invalid(format!(
                    "overlay pool '{pool_name}' references unknown backend name '{name}'"
                )));
            }
        } else if !overlay_backend.address.is_empty() {
            if let Some(base_backend) = base
                .iter_mut()
                .find(|b| b.address == overlay_backend.address)
            {
                apply_backend_overlay(base_backend, overlay_backend);
            } else {
                base.push(overlay_backend.clone());
            }
        } else {
            return Err(ConfigError::Invalid(format!(
                "overlay pool '{pool_name}' backend entry requires name or address"
            )));
        }
    }
    Ok(())
}

fn apply_backend_overlay(base: &mut Backend, overlay: &Backend) {
    if !overlay.address.is_empty() {
        base.address = overlay.address.clone();
    }
    if overlay.name.is_some() {
        base.name = overlay.name.clone();
    }
    if overlay.weight.is_some() {
        base.weight = overlay.weight;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::file::load_yaml;

    #[test]
    fn api_overlay_overrides_pool_weight() {
        let file_cfg =
            load_yaml(include_str!("../../../tests/fixtures/config/minimal.yaml")).unwrap();
        let mut overlay = file_cfg.clone();
        overlay.pools[0].backends[0].weight = Some(50);
        let merged = merge_file_and_overlay(&file_cfg, &overlay).unwrap();
        assert_eq!(merged.pools[0].backends[0].weight, Some(50));
        assert_eq!(file_cfg.pools[0].backends[0].weight, Some(100));
    }

    #[test]
    fn effective_config_without_overlay_returns_file() {
        let file_cfg =
            load_yaml(include_str!("../../../tests/fixtures/config/minimal.yaml")).unwrap();
        let effective = EffectiveConfig::new(file_cfg.clone());
        let merged = effective.effective();
        assert_eq!(merged.pools[0].backends[0].weight, Some(100));
    }

    #[test]
    fn clear_overlay_drops_api_layer() {
        let file_cfg =
            load_yaml(include_str!("../../../tests/fixtures/config/minimal.yaml")).unwrap();
        let mut effective = EffectiveConfig::new(file_cfg);
        let mut overlay = effective.file.clone();
        overlay.pools[0].backends[0].weight = Some(50);
        effective.overlay = Some(overlay);
        assert_eq!(effective.effective().pools[0].backends[0].weight, Some(50));
        clear_overlay(&mut effective);
        assert_eq!(effective.effective().pools[0].backends[0].weight, Some(100));
    }

    #[test]
    fn overlay_without_weight_preserves_file_backend_weight() {
        let file_cfg =
            load_yaml(include_str!("../../../tests/fixtures/config/minimal.yaml")).unwrap();
        let mut overlay = file_cfg.clone();
        overlay.pools[0].backends[0].weight = None;
        let merged = merge_file_and_overlay(&file_cfg, &overlay).unwrap();
        assert_eq!(merged.pools[0].backends[0].weight, Some(100));
    }

    #[test]
    fn overlay_replaces_entire_listeners_section() {
        let file_cfg =
            load_yaml(include_str!("../../../tests/fixtures/config/minimal.yaml")).unwrap();
        let mut overlay = file_cfg.clone();
        overlay.listeners = Some(conduit_proto::config::ListenersConfig {
            threads: 4,
            reuse_port: false,
            rcvbuf: 0,
            sndbuf: 0,
            listeners: vec![],
        });
        let merged = merge_file_and_overlay(&file_cfg, &overlay).unwrap();
        assert_eq!(merged.listeners.as_ref().unwrap().threads, 4);
        assert_eq!(merged.forward, file_cfg.forward);
    }

    #[test]
    fn is_overlay_patch_empty_for_schema_version_only() {
        let cfg = Config {
            schema_version: 1,
            ..Default::default()
        };
        assert!(is_overlay_patch_empty(&cfg));
    }

    #[test]
    fn merge_overlay_patches_accumulates_pool_and_listeners() {
        let file_cfg =
            load_yaml(include_str!("../../../tests/fixtures/config/minimal.yaml")).unwrap();
        let mut weight_patch = Config {
            schema_version: 1,
            pools: vec![{
                let mut pool = file_cfg.pools[0].clone();
                pool.backends[0].weight = Some(50);
                pool
            }],
            ..Default::default()
        };
        let listener_patch = Config {
            schema_version: 1,
            listeners: Some(conduit_proto::config::ListenersConfig {
                threads: 4,
                reuse_port: true,
                rcvbuf: 0,
                sndbuf: 0,
                listeners: vec![],
            }),
            ..Default::default()
        };
        let accumulated = merge_overlay_patches(&weight_patch, &listener_patch).unwrap();
        let effective = merge_file_and_overlay(&file_cfg, &accumulated).unwrap();
        assert_eq!(effective.pools[0].backends[0].weight, Some(50));
        assert_eq!(effective.listeners.as_ref().unwrap().threads, 4);
        weight_patch.pools[0].backends[0].weight = Some(99);
        let replaced = merge_overlay_patches(&weight_patch, &listener_patch).unwrap();
        assert_eq!(replaced.pools[0].backends[0].weight, Some(99));
    }

    #[test]
    fn overlay_patches_backend_by_name() {
        let file_yaml = r#"
schema_version: 1
listeners:
  threads: 1
  listeners:
    - address: "127.0.0.1:15353"
      protocol: udp
pools:
  - name: default
    backends:
      - name: resolver-east
        address: "127.0.0.1:5300"
"#;
        let file_cfg = load_yaml(file_yaml).unwrap();
        let overlay_yaml = r#"
schema_version: 1
pools:
  - name: default
    backends:
      - name: resolver-east
        weight: 10
"#;
        let overlay = load_yaml(overlay_yaml).unwrap();
        let merged = merge_file_and_overlay(&file_cfg, &overlay).unwrap();
        assert_eq!(merged.pools[0].backends[0].weight, Some(10));
        assert_eq!(merged.pools[0].backends[0].address, "127.0.0.1:5300");
    }

    #[test]
    fn overlay_rejects_unknown_backend_name() {
        let file_cfg =
            load_yaml(include_str!("../../../tests/fixtures/config/minimal.yaml")).unwrap();
        let overlay_yaml = r#"
schema_version: 1
pools:
  - name: default
    backends:
      - name: missing-backend
        weight: 10
"#;
        let overlay = load_yaml(overlay_yaml).unwrap();
        let err = merge_file_and_overlay(&file_cfg, &overlay).unwrap_err();
        assert!(err.to_string().contains("unknown backend name"));
    }

    #[test]
    fn overlay_address_fallback_still_appends_unknown() {
        let file_cfg =
            load_yaml(include_str!("../../../tests/fixtures/config/minimal.yaml")).unwrap();
        let overlay_yaml = r#"
schema_version: 1
pools:
  - name: default
    backends:
      - address: "127.0.0.1:5301"
        weight: 5
"#;
        let overlay = load_yaml(overlay_yaml).unwrap();
        let merged = merge_file_and_overlay(&file_cfg, &overlay).unwrap();
        assert_eq!(merged.pools[0].backends.len(), 2);
    }
}
