//! Dataplane and backend-name configuration tests.

#[cfg(test)]
mod tests {
    use crate::{
        effective_dataplane_runtime, load_yaml, merge_file_and_overlay, validate,
        DEFAULT_DATAPLANE_RUNTIME,
    };

    #[test]
    fn dataplane_sync_default_omitted_block() {
        let cfg = load_yaml(include_str!(
            "../../../tests/fixtures/config/dataplane-sync-default.yaml"
        ))
        .unwrap();
        assert!(cfg.dataplane.is_none());
        assert_eq!(effective_dataplane_runtime(&cfg), DEFAULT_DATAPLANE_RUNTIME);
        assert!(validate(&cfg).ok, "{:?}", validate(&cfg).errors);
    }

    #[test]
    fn dataplane_split_io_fixture_validates() {
        let cfg = load_yaml(include_str!(
            "../../../tests/fixtures/config/dataplane-split-io.yaml"
        ))
        .unwrap();
        assert_eq!(cfg.dataplane.as_ref().unwrap().runtime, "split_io");
        assert!(validate(&cfg).ok, "{:?}", validate(&cfg).errors);
    }

    #[test]
    fn reject_invalid_dataplane_runtime() {
        let cfg = load_yaml(include_str!(
            "../../../tests/fixtures/config/dataplane-invalid-runtime.yaml"
        ))
        .unwrap();
        let result = validate(&cfg);
        assert!(!result.ok);
        assert!(result
            .errors
            .iter()
            .any(|e| e.contains("dataplane.runtime")));
    }

    #[test]
    fn reject_duplicate_backend_names_in_pool() {
        let yaml = r#"
schema_version: 1
listeners:
  threads: 1
  listeners:
    - address: "127.0.0.1:15353"
      protocol: udp
pools:
  - name: default
    backends:
      - name: east
        address: "127.0.0.1:5300"
      - name: east
        address: "127.0.0.1:5301"
"#;
        let cfg = load_yaml(yaml).unwrap();
        let result = validate(&cfg);
        assert!(!result.ok);
        assert!(result
            .errors
            .iter()
            .any(|e| e.contains("duplicate backend name")));
    }

    #[test]
    fn overlay_merge_by_backend_name() {
        let file_cfg = load_yaml(include_str!(
            "../../../tests/fixtures/config/dataplane-named-backends.yaml"
        ))
        .unwrap();
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
    }
}
