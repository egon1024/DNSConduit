//! Synthesize an API overlay from a file baseline and a desired effective config.
//!
//! Invariant: `merge_file_and_overlay(file, synthesize_overlay(file, desired)?) == desired`
//! for every effective config reachable by supported primitive mutations (including
//! additive pool/backend removes).

use crate::error::ConfigError;
use conduit_proto::config::{Backend, Config, Pool};

/// Build an overlay `overlay'` such that merging it onto `file` yields `desired`.
pub fn synthesize_overlay(file: &Config, desired: &Config) -> Result<Config, ConfigError> {
    let mut overlay = Config {
        schema_version: desired.schema_version,
        ..Default::default()
    };

    synthesize_section_replace(file, desired, &mut overlay);
    overlay.pools = synthesize_pools(&file.pools, &desired.pools)?;

    // Extension points for later overlay-hot primitives (Phase D+). Each helper
    // currently no-ops when the section already matches via section-replace or
    // when deep-merge semantics need a dedicated synthesizer.
    synthesize_metrics_stub(file, desired, &mut overlay);
    synthesize_data_sources_stub(file, desired, &mut overlay);
    synthesize_caches_stub(file, desired, &mut overlay);

    Ok(overlay)
}

/// Section-replace fields: when desired differs from file, put the desired value
/// on the overlay (merge replaces the whole `Option` section when set).
fn synthesize_section_replace(file: &Config, desired: &Config, overlay: &mut Config) {
    if desired.listeners != file.listeners {
        overlay.listeners = desired.listeners.clone();
    }
    if desired.forward != file.forward {
        overlay.forward = desired.forward.clone();
    }
    if desired.orchestrator != file.orchestrator {
        overlay.orchestrator = desired.orchestrator;
    }
    if desired.events != file.events {
        overlay.events = desired.events.clone();
    }
    if desired.rhai != file.rhai {
        overlay.rhai = desired.rhai;
    }
    if desired.control != file.control {
        overlay.control = desired.control.clone();
    }
    if desired.logging != file.logging {
        overlay.logging = desired.logging.clone();
    }
    if desired.dataplane != file.dataplane {
        overlay.dataplane = desired.dataplane.clone();
    }
    if desired.shutdown != file.shutdown {
        overlay.shutdown = desired.shutdown;
    }
    if desired.lookup != file.lookup {
        overlay.lookup = desired.lookup.clone();
    }
    if desired.acls != file.acls {
        overlay.acls = desired.acls.clone();
    }
    if desired.data_source_limits != file.data_source_limits {
        overlay.data_source_limits = desired.data_source_limits;
    }
}

/// Metrics use deep-merge; a dedicated synthesizer lands with metrics primitives.
/// Best-effort: when unequal, place the full desired metrics on the overlay.
fn synthesize_metrics_stub(file: &Config, desired: &Config, overlay: &mut Config) {
    if desired.metrics != file.metrics {
        overlay.metrics = desired.metrics.clone();
    }
}

fn synthesize_data_sources_stub(file: &Config, desired: &Config, overlay: &mut Config) {
    if desired.data_sources != file.data_sources {
        overlay.data_sources = desired.data_sources.clone();
    }
}

fn synthesize_caches_stub(file: &Config, desired: &Config, overlay: &mut Config) {
    if desired.caches != file.caches {
        overlay.caches = desired.caches.clone();
    }
}

fn synthesize_pools(file: &[Pool], desired: &[Pool]) -> Result<Vec<Pool>, ConfigError> {
    let mut out = Vec::new();

    for file_pool in file {
        match desired.iter().find(|p| p.name == file_pool.name) {
            None => {
                out.push(Pool {
                    name: file_pool.name.clone(),
                    remove: Some(true),
                    ..Default::default()
                });
            }
            Some(desired_pool) => {
                let backends = synthesize_backends(&file_pool.backends, &desired_pool.backends)?;
                if !backends.is_empty() {
                    out.push(Pool {
                        name: file_pool.name.clone(),
                        backends,
                        ..Default::default()
                    });
                }
            }
        }
    }

    for desired_pool in desired {
        if !file.iter().any(|p| p.name == desired_pool.name) {
            let mut added = desired_pool.clone();
            added.remove = None;
            for b in &mut added.backends {
                b.remove = None;
            }
            out.push(added);
        }
    }

    Ok(out)
}

fn synthesize_backends(file: &[Backend], desired: &[Backend]) -> Result<Vec<Backend>, ConfigError> {
    let mut out = Vec::new();

    for file_b in file {
        match find_backend(desired, file_b) {
            None => {
                out.push(remove_marker_for(file_b));
            }
            Some(desired_b) => {
                if let Some(delta) = backend_delta(file_b, desired_b) {
                    out.push(delta);
                }
            }
        }
    }

    for desired_b in desired {
        if find_backend(file, desired_b).is_none() {
            let mut added = desired_b.clone();
            added.remove = None;
            out.push(added);
        }
    }

    Ok(out)
}

fn find_backend<'a>(haystack: &'a [Backend], needle: &Backend) -> Option<&'a Backend> {
    if let Some(name) = needle.name.as_ref().filter(|n| !n.is_empty()) {
        if let Some(b) = haystack
            .iter()
            .find(|b| b.name.as_deref() == Some(name.as_str()))
        {
            return Some(b);
        }
    }
    if !needle.address.is_empty() {
        return haystack.iter().find(|b| b.address == needle.address);
    }
    None
}

fn remove_marker_for(b: &Backend) -> Backend {
    if let Some(name) = b.name.as_ref().filter(|n| !n.is_empty()) {
        Backend {
            name: Some(name.clone()),
            remove: Some(true),
            ..Default::default()
        }
    } else {
        Backend {
            address: b.address.clone(),
            remove: Some(true),
            ..Default::default()
        }
    }
}

fn backend_delta(file_b: &Backend, desired_b: &Backend) -> Option<Backend> {
    let mut delta = Backend::default();
    let mut changed = false;

    if let Some(name) = desired_b.name.as_ref().filter(|n| !n.is_empty()) {
        delta.name = Some(name.clone());
    } else if !desired_b.address.is_empty() {
        delta.address = desired_b.address.clone();
    } else if let Some(name) = file_b.name.as_ref().filter(|n| !n.is_empty()) {
        delta.name = Some(name.clone());
    } else {
        delta.address = file_b.address.clone();
    }

    if desired_b.address != file_b.address && !desired_b.address.is_empty() {
        delta.address = desired_b.address.clone();
        changed = true;
    }
    if desired_b.name != file_b.name {
        delta.name = desired_b.name.clone();
        changed = true;
    }
    if desired_b.weight != file_b.weight {
        delta.weight = desired_b.weight;
        changed = true;
    }
    if desired_b.probe_qname != file_b.probe_qname {
        delta.probe_qname = desired_b.probe_qname.clone();
        changed = true;
    }
    if desired_b.probe_qtype != file_b.probe_qtype {
        delta.probe_qtype = desired_b.probe_qtype.clone();
        changed = true;
    }
    if desired_b.probe_source != file_b.probe_source {
        delta.probe_source = desired_b.probe_source.clone();
        changed = true;
    }
    if desired_b.transport != file_b.transport {
        delta.transport = desired_b.transport.clone();
        changed = true;
    }

    if changed {
        Some(delta)
    } else {
        None
    }
}

/// Debug helper used by unit tests: assert the synthesize round-trip invariant.
#[cfg(test)]
pub(crate) fn assert_round_trip(file: &Config, desired: &Config) {
    use crate::merge::merge_file_and_overlay;
    let overlay = synthesize_overlay(file, desired).expect("synthesize");
    let merged = merge_file_and_overlay(file, &overlay).expect("merge");
    assert_eq!(
        merged, *desired,
        "round-trip failed\noverlay={overlay:#?}\nmerged={merged:#?}\ndesired={desired:#?}"
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::file::load_yaml;
    use crate::merge::merge_file_and_overlay;

    fn two_backend_file() -> Config {
        load_yaml(
            r#"
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
        weight: 100
      - name: secondary
        address: "127.0.0.1:5301"
        weight: 100
"#,
        )
        .unwrap()
    }

    #[test]
    fn synthesize_weight_only_round_trip() {
        let file = two_backend_file();
        let mut desired = file.clone();
        desired.pools[0].backends[0].weight = Some(50);
        assert_round_trip(&file, &desired);
    }

    #[test]
    fn synthesize_backend_add_round_trip() {
        let file = two_backend_file();
        let mut desired = file.clone();
        desired.pools[0].backends.push(Backend {
            name: Some("tertiary".into()),
            address: "127.0.0.1:5302".into(),
            weight: Some(10),
            ..Default::default()
        });
        assert_round_trip(&file, &desired);
    }

    #[test]
    fn synthesize_backend_remove_round_trip() {
        let file = two_backend_file();
        let mut desired = file.clone();
        desired.pools[0]
            .backends
            .retain(|b| b.name.as_deref() != Some("secondary"));
        assert_round_trip(&file, &desired);
        let overlay = synthesize_overlay(&file, &desired).unwrap();
        assert!(overlay.pools[0]
            .backends
            .iter()
            .any(|b| b.remove == Some(true) && b.name.as_deref() == Some("secondary")));
        let merged = merge_file_and_overlay(&file, &overlay).unwrap();
        assert_eq!(merged.pools[0].backends.len(), 1);
        assert_eq!(merged.pools[0].backends[0].name.as_deref(), Some("primary"));
    }

    #[test]
    fn synthesize_preserves_unrelated_when_only_pools_change() {
        let file = two_backend_file();
        let mut desired = file.clone();
        desired.pools[0].backends[1].weight = Some(25);
        let overlay = synthesize_overlay(&file, &desired).unwrap();
        assert!(overlay.listeners.is_none());
        assert_round_trip(&file, &desired);
    }
}
