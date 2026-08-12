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
//!   Unknown overlay backend `name` without an `address` is rejected (typo on
//!   update). Unknown `name` with an `address` appends a new named backend.
//!   Overlay backends matched only by `address` that are not present in a pool
//!   are appended. Explicit `remove: true` on a pool or backend (overlay-only
//!   marker) deletes the matched entry when merging onto the file baseline;
//!   unknown remove targets fail that apply. When accumulating overlay patches
//!   (`merge_overlay_patches`), remove markers are retained so a later file
//!   merge can delete file-layer members absent from the sparse overlay.
//!   Sparse update/append without the marker is unchanged.
//! - **`metrics`**: deep merge (intentional exception to section replace). Nested
//!   maps merge by key; scalars win when set; `categories.include`/`exclude`
//!   replace only when `include_set`/`exclude_set`; `user_metrics` match-by-name.

use crate::error::ConfigError;
use conduit_proto::config::{
    Backend, Config, MetricsCollectEmit, MetricsConfig, OtelMetricsConfig, Pool,
    PrometheusMetricsConfig, UserMetricExportConfig,
};

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

/// Merge one overlay patch into another (same rules as [`merge_file_and_overlay`],
/// except remove markers are **accumulated** rather than applied: prior overlay
/// entries for the same pool/backend identity are dropped and the `remove: true`
/// marker is retained so a later [`merge_file_and_overlay`] against the file
/// baseline can delete file-layer members).
pub fn merge_overlay_patches(base: &Config, patch: &Config) -> Result<Config, ConfigError> {
    merge_configs(base, patch, MergeMode::AccumulateOverlay)
}

#[derive(Clone, Copy)]
enum MergeMode {
    /// Overlay onto file (or any full baseline): apply removes; unknown targets fail.
    ApplyToBaseline,
    /// Overlay onto overlay: keep remove markers for a later baseline apply.
    AccumulateOverlay,
}

fn merge_configs(base: &Config, overlay: &Config, mode: MergeMode) -> Result<Config, ConfigError> {
    let mut merged = base.clone();

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
    if overlay.data_source_limits.is_some() {
        merged.data_source_limits = overlay.data_source_limits;
    }
    if overlay.lookup.is_some() {
        merged.lookup = overlay.lookup.clone();
    }
    if overlay.acls.is_some() {
        merged.acls = overlay.acls.clone();
    }
    if !overlay.caches.is_empty() {
        merged.caches = overlay.caches.clone();
    }

    if let Some(overlay_metrics) = &overlay.metrics {
        merged.metrics = Some(merge_metrics(merged.metrics.as_ref(), overlay_metrics));
    }

    if !overlay.pools.is_empty() {
        merge_pools(&mut merged.pools, &overlay.pools, mode)?;
    }

    Ok(merged)
}

pub fn merge_file_and_overlay(file: &Config, overlay: &Config) -> Result<Config, ConfigError> {
    merge_configs(file, overlay, MergeMode::ApplyToBaseline)
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
        && cfg.data_source_limits.is_none()
        && cfg.lookup.is_none()
        && cfg.caches.is_empty()
        && cfg.acls.is_none()
        && cfg.metrics.is_none()
}

fn merge_pools(base: &mut Vec<Pool>, overlay: &[Pool], mode: MergeMode) -> Result<(), ConfigError> {
    for overlay_pool in overlay {
        if overlay_pool.remove.unwrap_or(false) {
            match mode {
                MergeMode::ApplyToBaseline => {
                    let name = &overlay_pool.name;
                    if name.is_empty() {
                        return Err(ConfigError::Invalid(
                            "overlay pool remove requires a non-empty name".into(),
                        ));
                    }
                    let before = base.len();
                    base.retain(|p| p.name != *name);
                    if base.len() == before {
                        return Err(ConfigError::Invalid(format!(
                            "overlay requests remove of unknown pool '{name}'"
                        )));
                    }
                }
                MergeMode::AccumulateOverlay => {
                    let name = &overlay_pool.name;
                    if name.is_empty() {
                        return Err(ConfigError::Invalid(
                            "overlay pool remove requires a non-empty name".into(),
                        ));
                    }
                    // Drop prior overlay state for this pool; keep the remove marker.
                    base.retain(|p| p.name != *name);
                    base.push(Pool {
                        name: name.clone(),
                        remove: Some(true),
                        ..Default::default()
                    });
                }
            }
            continue;
        }
        if let Some(base_pool) = base.iter_mut().find(|p| p.name == overlay_pool.name) {
            // A later non-remove patch for a pool cancels a prior pool-level remove marker.
            base_pool.remove = None;
            merge_backends(
                &base_pool.name,
                &mut base_pool.backends,
                &overlay_pool.backends,
                mode,
            )?;
        } else {
            let mut added = overlay_pool.clone();
            match mode {
                MergeMode::ApplyToBaseline => {
                    // Never leave remove markers on effective config.
                    added.remove = None;
                    for b in &mut added.backends {
                        b.remove = None;
                    }
                }
                MergeMode::AccumulateOverlay => {
                    // Keep backend remove markers inside the accumulated overlay.
                    added.remove = None;
                }
            }
            base.push(added);
        }
    }
    Ok(())
}

fn merge_backends(
    pool_name: &str,
    base: &mut Vec<Backend>,
    overlay: &[Backend],
    mode: MergeMode,
) -> Result<(), ConfigError> {
    for overlay_backend in overlay {
        if overlay_backend.remove.unwrap_or(false) {
            match mode {
                MergeMode::ApplyToBaseline => {
                    remove_backend(pool_name, base, overlay_backend)?;
                }
                MergeMode::AccumulateOverlay => {
                    accumulate_backend_remove(pool_name, base, overlay_backend)?;
                }
            }
            continue;
        }
        if let Some(name) = overlay_backend.name.as_ref().filter(|n| !n.is_empty()) {
            if let Some(base_backend) = base
                .iter_mut()
                .find(|b| b.name.as_deref() == Some(name.as_str()))
            {
                apply_backend_overlay(base_backend, overlay_backend);
                // A later update cancels a prior remove marker for this identity.
                base_backend.remove = None;
            } else if !overlay_backend.address.is_empty() {
                // Named add: unknown name with an address appends a new backend.
                // Unknown name without address remains a hard error (typo on update).
                let mut added = overlay_backend.clone();
                added.remove = None;
                base.push(added);
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
                base_backend.remove = None;
            } else {
                let mut added = overlay_backend.clone();
                added.remove = None;
                base.push(added);
            }
        } else {
            return Err(ConfigError::Invalid(format!(
                "overlay pool '{pool_name}' backend entry requires name or address"
            )));
        }
    }
    Ok(())
}

/// Retain a remove marker in an accumulated overlay; drop prior entries for the same identity.
fn accumulate_backend_remove(
    pool_name: &str,
    base: &mut Vec<Backend>,
    overlay_backend: &Backend,
) -> Result<(), ConfigError> {
    if let Some(name) = overlay_backend.name.as_ref().filter(|n| !n.is_empty()) {
        base.retain(|b| b.name.as_deref() != Some(name.as_str()));
        base.push(Backend {
            name: Some(name.clone()),
            remove: Some(true),
            ..Default::default()
        });
        return Ok(());
    }
    if !overlay_backend.address.is_empty() {
        let addr = overlay_backend.address.clone();
        base.retain(|b| b.address != addr);
        base.push(Backend {
            address: addr,
            remove: Some(true),
            ..Default::default()
        });
        return Ok(());
    }
    Err(ConfigError::Invalid(format!(
        "overlay pool '{pool_name}' backend remove requires name or address"
    )))
}

fn remove_backend(
    pool_name: &str,
    base: &mut Vec<Backend>,
    overlay_backend: &Backend,
) -> Result<(), ConfigError> {
    if let Some(name) = overlay_backend.name.as_ref().filter(|n| !n.is_empty()) {
        let before = base.len();
        base.retain(|b| b.name.as_deref() != Some(name.as_str()));
        if base.len() == before {
            return Err(ConfigError::Invalid(format!(
                "overlay pool '{pool_name}' requests remove of unknown backend name '{name}'"
            )));
        }
        return Ok(());
    }
    if !overlay_backend.address.is_empty() {
        let addr = &overlay_backend.address;
        let before = base.len();
        base.retain(|b| b.address != *addr);
        if base.len() == before {
            return Err(ConfigError::Invalid(format!(
                "overlay pool '{pool_name}' requests remove of unknown backend address '{addr}'"
            )));
        }
        return Ok(());
    }
    Err(ConfigError::Invalid(format!(
        "overlay pool '{pool_name}' backend remove requires name or address"
    )))
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
    // remove is overlay-only; never copy onto effective backends.
}

/// Deep-merge overlay metrics into a file baseline (or overlay-alone when base is None).
fn merge_metrics(base: Option<&MetricsConfig>, overlay: &MetricsConfig) -> MetricsConfig {
    let mut merged = base.cloned().unwrap_or_default();

    if overlay.enabled.is_some() {
        merged.enabled = overlay.enabled;
    }
    if !overlay.profile.is_empty() {
        merged.profile = overlay.profile.clone();
    }
    if !overlay.base.is_empty() {
        merged.base = overlay.base.clone();
    }

    for (key, overlay_ce) in &overlay.collection {
        merged
            .collection
            .entry(key.clone())
            .and_modify(|base_ce| merge_collect_emit(base_ce, overlay_ce))
            .or_insert_with(|| *overlay_ce);
    }

    if let Some(overlay_g) = &overlay.granularity {
        let mut g = merged.granularity.unwrap_or_default();
        if !overlay_g.default.is_empty() {
            g.default = overlay_g.default.clone();
        }
        for (family, dims) in &overlay_g.overrides {
            g.overrides.insert(family.clone(), dims.clone());
        }
        merged.granularity = Some(g);
    }

    if let Some(overlay_cats) = &overlay.categories {
        let mut cats = merged.categories.unwrap_or_default();
        if overlay_cats.include_set {
            cats.include = overlay_cats.include.clone();
            cats.include_set = true;
        }
        if overlay_cats.exclude_set {
            cats.exclude = overlay_cats.exclude.clone();
            cats.exclude_set = true;
        }
        merged.categories = Some(cats);
    }

    if let Some(overlay_ee) = &overlay.event_export {
        let mut ee = merged.event_export.unwrap_or_default();
        if overlay_ee.collect.is_some() {
            ee.collect = overlay_ee.collect;
        }
        if overlay_ee.emit.is_some() {
            ee.emit = overlay_ee.emit;
        }
        merged.event_export = Some(ee);
    }

    if let Some(overlay_prom) = &overlay.prometheus {
        merged.prometheus = Some(merge_prometheus(merged.prometheus.as_ref(), overlay_prom));
    }

    if let Some(overlay_otel) = &overlay.otel {
        merged.otel = Some(merge_otel(merged.otel.as_ref(), overlay_otel));
    }

    merge_user_metrics(&mut merged.user_metrics, &overlay.user_metrics);

    merged
}

fn merge_collect_emit(base: &mut MetricsCollectEmit, overlay: &MetricsCollectEmit) {
    if overlay.collect.is_some() {
        base.collect = overlay.collect;
    }
    if overlay.emit.is_some() {
        base.emit = overlay.emit;
    }
}

fn merge_prometheus(
    base: Option<&PrometheusMetricsConfig>,
    overlay: &PrometheusMetricsConfig,
) -> PrometheusMetricsConfig {
    let mut merged = base.cloned().unwrap_or_default();
    if !overlay.listen_address.is_empty() {
        merged.listen_address = overlay.listen_address.clone();
    }
    if !overlay.path.is_empty() {
        merged.path = overlay.path.clone();
    }
    merged
}

fn merge_otel(base: Option<&OtelMetricsConfig>, overlay: &OtelMetricsConfig) -> OtelMetricsConfig {
    let mut merged = base.cloned().unwrap_or_default();
    if !overlay.endpoint.is_empty() {
        merged.endpoint = overlay.endpoint.clone();
    }
    if overlay.push_interval_ms != 0 {
        merged.push_interval_ms = overlay.push_interval_ms;
    }
    if overlay.allow_invalid_certs.is_some() {
        merged.allow_invalid_certs = overlay.allow_invalid_certs;
    }
    for (k, v) in &overlay.resource_attributes {
        merged.resource_attributes.insert(k.clone(), v.clone());
    }
    for (k, v) in &overlay.headers {
        merged.headers.insert(k.clone(), v.clone());
    }
    merged
}

fn merge_user_metrics(base: &mut Vec<UserMetricExportConfig>, overlay: &[UserMetricExportConfig]) {
    for overlay_entry in overlay {
        if let Some(base_entry) = base.iter_mut().find(|u| u.name == overlay_entry.name) {
            if !overlay_entry.export.is_empty() {
                base_entry.export = overlay_entry.export.clone();
            }
            if overlay_entry.collect.is_some() {
                base_entry.collect = overlay_entry.collect;
            }
            if overlay_entry.emit.is_some() {
                base_entry.emit = overlay_entry.emit;
            }
            if !overlay_entry.help.is_empty() {
                base_entry.help = overlay_entry.help.clone();
            }
        } else {
            base.push(overlay_entry.clone());
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
    fn overlay_named_backend_with_address_appends() {
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
      - name: primary
        address: "127.0.0.1:5300"
"#;
        let file_cfg = load_yaml(file_yaml).unwrap();
        let overlay = load_yaml(
            r#"
schema_version: 1
pools:
  - name: default
    backends:
      - name: secondary
        address: "127.0.0.1:5301"
        weight: 50
"#,
        )
        .unwrap();
        let merged = merge_file_and_overlay(&file_cfg, &overlay).unwrap();
        assert_eq!(merged.pools[0].backends.len(), 2);
        assert_eq!(
            merged.pools[0].backends[1].name.as_deref(),
            Some("secondary")
        );
        assert_eq!(merged.pools[0].backends[1].weight, Some(50));
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

    #[test]
    fn sparse_weight_patch_unchanged_with_named_peer() {
        let file_yaml = r#"
schema_version: 1
listeners:
  threads: 1
  listeners:
    - address: "127.0.0.1:15353"
      protocol: udp
pools:
  - name: edge
    backends:
      - name: a
        address: "127.0.0.1:5300"
        weight: 100
      - name: b
        address: "127.0.0.1:5301"
        weight: 100
"#;
        let file_cfg = load_yaml(file_yaml).unwrap();
        let overlay = load_yaml(
            r#"
schema_version: 1
pools:
  - name: edge
    backends:
      - name: a
        weight: 10
"#,
        )
        .unwrap();
        let merged = merge_file_and_overlay(&file_cfg, &overlay).unwrap();
        assert_eq!(merged.pools[0].backends.len(), 2);
        assert_eq!(merged.pools[0].backends[0].weight, Some(10));
        assert_eq!(merged.pools[0].backends[1].weight, Some(100));
    }

    #[test]
    fn explicit_backend_remove_by_name() {
        let file_yaml = r#"
schema_version: 1
listeners:
  threads: 1
  listeners:
    - address: "127.0.0.1:15353"
      protocol: udp
pools:
  - name: edge
    backends:
      - name: a
        address: "127.0.0.1:5300"
      - name: b
        address: "127.0.0.1:5301"
"#;
        let file_cfg = load_yaml(file_yaml).unwrap();
        let overlay = load_yaml(
            r#"
schema_version: 1
pools:
  - name: edge
    backends:
      - name: b
        remove: true
"#,
        )
        .unwrap();
        let merged = merge_file_and_overlay(&file_cfg, &overlay).unwrap();
        assert_eq!(merged.pools[0].backends.len(), 1);
        assert_eq!(merged.pools[0].backends[0].name.as_deref(), Some("a"));
        assert!(merged.pools[0].backends.iter().all(|b| b.remove.is_none()));
    }

    #[test]
    fn accumulate_remove_after_sparse_weight_keeps_marker() {
        // Reproduces: weight apply then remove of a file-layer peer that was never
        // in the sparse overlay — must not fail during overlay accumulation.
        let file_yaml = r#"
schema_version: 1
listeners:
  threads: 1
  listeners:
    - address: "127.0.0.1:15353"
      protocol: udp
pools:
  - name: edge
    backends:
      - name: primary
        address: "127.0.0.1:5300"
        weight: 100
      - name: secondary
        address: "127.0.0.1:5301"
        weight: 100
"#;
        let file_cfg = load_yaml(file_yaml).unwrap();
        let weight = load_yaml(
            r#"
schema_version: 1
pools:
  - name: edge
    backends:
      - name: primary
        weight: 50
"#,
        )
        .unwrap();
        let remove = load_yaml(
            r#"
schema_version: 1
pools:
  - name: edge
    backends:
      - name: secondary
        remove: true
"#,
        )
        .unwrap();
        let accumulated = merge_overlay_patches(&weight, &remove).unwrap();
        assert!(
            accumulated.pools[0]
                .backends
                .iter()
                .any(|b| b.name.as_deref() == Some("secondary") && b.remove == Some(true)),
            "remove marker must be retained in accumulated overlay: {accumulated:?}"
        );
        let merged = merge_file_and_overlay(&file_cfg, &accumulated).unwrap();
        assert_eq!(merged.pools[0].backends.len(), 1);
        assert_eq!(merged.pools[0].backends[0].name.as_deref(), Some("primary"));
        assert_eq!(merged.pools[0].backends[0].weight, Some(50));
    }

    #[test]
    fn remove_unknown_backend_fails() {
        let file_cfg =
            load_yaml(include_str!("../../../tests/fixtures/config/minimal.yaml")).unwrap();
        let overlay = load_yaml(
            r#"
schema_version: 1
pools:
  - name: default
    backends:
      - name: missing
        remove: true
"#,
        )
        .unwrap();
        let err = merge_file_and_overlay(&file_cfg, &overlay).unwrap_err();
        assert!(err.to_string().contains("unknown backend name"));
    }

    #[test]
    fn export_after_remove_has_no_tombstones() {
        use crate::export::export_yaml;

        let file_yaml = r#"
schema_version: 1
listeners:
  threads: 1
  listeners:
    - address: "127.0.0.1:15353"
      protocol: udp
pools:
  - name: edge
    backends:
      - name: a
        address: "127.0.0.1:5300"
      - name: b
        address: "127.0.0.1:5301"
"#;
        let file_cfg = load_yaml(file_yaml).unwrap();
        let overlay = load_yaml(
            r#"
schema_version: 1
pools:
  - name: edge
    backends:
      - name: b
        remove: true
"#,
        )
        .unwrap();
        let merged = merge_file_and_overlay(&file_cfg, &overlay).unwrap();
        let yaml_out = export_yaml(&merged).unwrap();
        assert!(!yaml_out.contains("remove:"));
        assert!(yaml_out.contains("name: a"));
        assert!(!yaml_out.contains("name: b"));
    }

    #[test]
    fn metrics_overlay_exclude_keeps_baseline_include_and_fields() {
        use conduit_proto::config::{MetricsCategories, MetricsCollectEmit};

        let file = Config {
            schema_version: 1,
            metrics: Some(MetricsConfig {
                enabled: Some(true),
                base: "standard".into(),
                categories: Some(MetricsCategories {
                    include: vec!["timing".into()],
                    exclude: vec![],
                    include_set: true,
                    exclude_set: false,
                }),
                collection: [(
                    "timing".into(),
                    MetricsCollectEmit {
                        collect: Some(true),
                        emit: Some(true),
                    },
                )]
                .into_iter()
                .collect(),
                ..Default::default()
            }),
            ..Default::default()
        };
        let overlay = Config {
            schema_version: 1,
            metrics: Some(MetricsConfig {
                categories: Some(MetricsCategories {
                    include: vec![],
                    exclude: vec!["process".into()],
                    include_set: false,
                    exclude_set: true,
                }),
                ..Default::default()
            }),
            ..Default::default()
        };
        let merged = merge_file_and_overlay(&file, &overlay).unwrap();
        let m = merged.metrics.as_ref().unwrap();
        assert_eq!(m.enabled, Some(true));
        assert_eq!(m.base, "standard");
        let cats = m.categories.as_ref().unwrap();
        assert_eq!(cats.include, vec!["timing".to_string()]);
        assert!(cats.include_set);
        assert_eq!(cats.exclude, vec!["process".to_string()]);
        assert!(cats.exclude_set);
        let timing = m.collection.get("timing").unwrap();
        assert_eq!(timing.collect, Some(true));
        assert_eq!(timing.emit, Some(true));
    }

    #[test]
    fn metrics_collection_deep_merges_by_category() {
        use conduit_proto::config::MetricsCollectEmit;

        let file = Config {
            schema_version: 1,
            metrics: Some(MetricsConfig {
                enabled: Some(true),
                collection: [(
                    "timing".into(),
                    MetricsCollectEmit {
                        collect: Some(true),
                        emit: Some(false),
                    },
                )]
                .into_iter()
                .collect(),
                ..Default::default()
            }),
            ..Default::default()
        };
        let overlay = Config {
            schema_version: 1,
            metrics: Some(MetricsConfig {
                collection: [(
                    "process".into(),
                    MetricsCollectEmit {
                        collect: None,
                        emit: Some(false),
                    },
                )]
                .into_iter()
                .collect(),
                ..Default::default()
            }),
            ..Default::default()
        };
        let merged = merge_file_and_overlay(&file, &overlay).unwrap();
        let coll = &merged.metrics.as_ref().unwrap().collection;
        let timing = coll.get("timing").unwrap();
        assert_eq!(timing.emit, Some(false));
        assert_eq!(timing.collect, Some(true));
        let process = coll.get("process").unwrap();
        assert_eq!(process.emit, Some(false));
    }

    #[test]
    fn metrics_only_overlay_is_not_empty_patch() {
        let cfg = Config {
            schema_version: 1,
            metrics: Some(MetricsConfig {
                enabled: Some(false),
                ..Default::default()
            }),
            ..Default::default()
        };
        assert!(!is_overlay_patch_empty(&cfg));
    }

    #[test]
    fn metrics_user_metrics_match_by_name_and_append() {
        let file = Config {
            schema_version: 1,
            metrics: Some(MetricsConfig {
                enabled: Some(true),
                user_metrics: vec![UserMetricExportConfig {
                    name: "hits".into(),
                    export: "full".into(),
                    collect: Some(true),
                    emit: Some(true),
                    help: String::new(),
                }],
                ..Default::default()
            }),
            ..Default::default()
        };
        let overlay = Config {
            schema_version: 1,
            metrics: Some(MetricsConfig {
                user_metrics: vec![
                    UserMetricExportConfig {
                        name: "hits".into(),
                        export: String::new(),
                        collect: Some(true),
                        emit: Some(false),
                        help: String::new(),
                    },
                    UserMetricExportConfig {
                        name: "misses".into(),
                        export: String::new(),
                        collect: Some(true),
                        emit: Some(true),
                        help: String::new(),
                    },
                ],
                ..Default::default()
            }),
            ..Default::default()
        };
        let merged = merge_file_and_overlay(&file, &overlay).unwrap();
        let users = &merged.metrics.as_ref().unwrap().user_metrics;
        assert_eq!(users.len(), 2);
        assert_eq!(users[0].name, "hits");
        assert_eq!(users[0].export, "full");
        assert_eq!(users[0].collect, Some(true));
        assert_eq!(users[0].emit, Some(false));
        assert_eq!(users[1].name, "misses");
        assert_eq!(users[1].collect, Some(true));
        assert_eq!(users[1].emit, Some(true));
    }

    #[test]
    fn metrics_user_metrics_merge_help_by_name() {
        let file = Config {
            schema_version: 1,
            metrics: Some(MetricsConfig {
                enabled: Some(true),
                user_metrics: vec![UserMetricExportConfig {
                    name: "hits".into(),
                    export: String::new(),
                    collect: Some(true),
                    emit: Some(true),
                    help: "from file".into(),
                }],
                ..Default::default()
            }),
            ..Default::default()
        };
        let overlay = Config {
            schema_version: 1,
            metrics: Some(MetricsConfig {
                user_metrics: vec![UserMetricExportConfig {
                    name: "hits".into(),
                    export: String::new(),
                    collect: None,
                    emit: None,
                    help: "from overlay".into(),
                }],
                ..Default::default()
            }),
            ..Default::default()
        };
        let merged = merge_file_and_overlay(&file, &overlay).unwrap();
        let users = &merged.metrics.as_ref().unwrap().user_metrics;
        assert_eq!(users[0].help, "from overlay");
        assert_eq!(users[0].collect, Some(true));
        assert_eq!(users[0].emit, Some(true));
    }
}
