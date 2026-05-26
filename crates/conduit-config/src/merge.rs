//! File baseline + API overlay merge (spec §5.2).
//!
//! # Phase 0 merge strategy
//!
//! - **Top-level sections** (`listeners`, `forward`, `orchestrator`, `observation`,
//!   `rhai`, `control`): if the overlay has the section set (`Option::is_some`), replace
//!   the entire sub-message on the effective config.
//! - **`schema_version`**: overlay value wins.
//! - **`pools`**: match pools by `name`; for each overlay pool, match backends by
//!   `address` and update fields. Overlay pools not present in the file are appended.
//!   Overlay backends not present in a pool are appended.

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
            Some(overlay) => merge_file_and_overlay(&self.file, overlay),
            None => self.file.clone(),
        }
    }
}

pub fn merge_file_and_overlay(file: &Config, overlay: &Config) -> Config {
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
    if overlay.observation.is_some() {
        merged.observation = overlay.observation.clone();
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
    if !overlay.data_sources.is_empty() {
        merged.data_sources = overlay.data_sources.clone();
    }

    if !overlay.pools.is_empty() {
        merge_pools(&mut merged.pools, &overlay.pools);
    }

    merged
}

pub fn clear_overlay(effective: &mut EffectiveConfig) {
    effective.overlay = None;
}

fn merge_pools(base: &mut Vec<Pool>, overlay: &[Pool]) {
    for overlay_pool in overlay {
        if let Some(base_pool) = base.iter_mut().find(|p| p.name == overlay_pool.name) {
            merge_backends(&mut base_pool.backends, &overlay_pool.backends);
        } else {
            base.push(overlay_pool.clone());
        }
    }
}

fn merge_backends(base: &mut Vec<Backend>, overlay: &[Backend]) {
    for overlay_backend in overlay {
        if let Some(base_backend) = base
            .iter_mut()
            .find(|b| b.address == overlay_backend.address)
        {
            // Optional weight: unset in overlay does not clear the file-layer value.
            if overlay_backend.weight.is_some() {
                base_backend.weight = overlay_backend.weight;
            }
        } else {
            base.push(overlay_backend.clone());
        }
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
        let merged = merge_file_and_overlay(&file_cfg, &overlay);
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
        let merged = merge_file_and_overlay(&file_cfg, &overlay);
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
        let merged = merge_file_and_overlay(&file_cfg, &overlay);
        assert_eq!(merged.listeners.as_ref().unwrap().threads, 4);
        assert_eq!(merged.forward, file_cfg.forward);
    }
}
