//! ACL + cidr data source config: load, validate, overlay replace, export (no prefix dump).

use conduit_config::{
    export_yaml, is_overlay_patch_empty, load_overlay_patch, load_yaml, merge_file_and_overlay,
    validate,
};
use std::io::Write;
use tempfile::NamedTempFile;

fn minimal_with_cidr_and_acls(cidr_path: &str) -> String {
    format!(
        r#"
schema_version: 1
listeners:
  listeners:
    - address: "127.0.0.1:15353"
      protocol: udp
      name: dns
pools:
  - name: primary
    backends:
      - address: "127.0.0.1:5300"
        weight: 100
data_sources:
  - name: corp_nets
    type: cidr
    path: {cidr_path}
acls:
  default_action: deny
  rules:
    - match: corp_nets
      action: accept
"#
    )
}

#[test]
fn acls_and_cidr_load_validate_and_export_without_prefix_contents() {
    let mut file = NamedTempFile::new().unwrap();
    writeln!(file, "10.0.0.0/8").unwrap();
    writeln!(file, "2001:db8::/32").unwrap();
    let path = file.path().to_str().unwrap();

    let cfg = load_yaml(&minimal_with_cidr_and_acls(path)).expect("load");
    let v = validate(&cfg);
    assert!(v.ok, "{:?}", v.errors);

    let acls = cfg.acls.as_ref().expect("acls");
    assert_eq!(acls.default_action, "deny");
    assert_eq!(acls.rules.len(), 1);
    assert_eq!(acls.rules[0].r#match, "corp_nets");
    assert_eq!(acls.rules[0].action, "accept");
    assert_eq!(cfg.data_sources[0].r#type, "cidr");

    let out = export_yaml(&cfg).expect("export");
    assert!(out.contains("type: cidr"));
    assert!(out.contains("corp_nets"));
    assert!(out.contains(&format!("path: {path}")) || out.contains("path:"));
    // Export must not inline file prefixes.
    assert!(!out.contains("10.0.0.0/8"));
    assert!(!out.contains("2001:db8::/32"));

    let cfg2 = load_yaml(&out).expect("reload export");
    assert!(validate(&cfg2).ok);
    assert_eq!(
        cfg2.acls.as_ref().unwrap().default_action,
        cfg.acls.as_ref().unwrap().default_action
    );
}

#[test]
fn overlay_replaces_top_level_acls() {
    let mut file = NamedTempFile::new().unwrap();
    writeln!(file, "10.0.0.0/8").unwrap();
    let path = file.path().to_str().unwrap();
    let file_cfg = load_yaml(&minimal_with_cidr_and_acls(path)).unwrap();

    let overlay = load_overlay_patch(
        r#"
schema_version: 1
acls:
  default_action: allow
  rules: []
"#,
    )
    .expect("overlay");
    assert!(!is_overlay_patch_empty(&overlay));
    let merged = merge_file_and_overlay(&file_cfg, &overlay).unwrap();
    let acls = merged.acls.as_ref().unwrap();
    assert_eq!(acls.default_action, "allow");
    assert!(acls.rules.is_empty());
}

#[test]
fn reject_acl_match_not_cidr_source() {
    let yaml = r#"
schema_version: 1
listeners:
  listeners:
    - address: "127.0.0.1:15353"
      protocol: udp
pools:
  - name: primary
    backends:
      - address: "127.0.0.1:5300"
data_sources:
  - name: blocklist
    type: csv
    path: /tmp/x.csv
acls:
  default_action: allow
  rules:
    - match: blocklist
      action: drop
"#;
    let cfg = load_yaml(yaml).unwrap();
    let v = validate(&cfg);
    assert!(!v.ok);
    assert!(v
        .errors
        .iter()
        .any(|e| e.contains("type:cidr") || e.contains("cidr")));
}

#[test]
fn reject_tag_action_without_tag_name() {
    let mut file = NamedTempFile::new().unwrap();
    writeln!(file, "10.0.0.0/8").unwrap();
    let path = file.path().to_str().unwrap();
    let yaml = format!(
        r#"
schema_version: 1
listeners:
  listeners:
    - address: "127.0.0.1:15353"
      protocol: udp
pools:
  - name: primary
    backends:
      - address: "127.0.0.1:5300"
data_sources:
  - name: corp_nets
    type: cidr
    path: {path}
acls:
  default_action: allow
  rules:
    - match: corp_nets
      action: tag
"#
    );
    let cfg = load_yaml(&yaml).unwrap();
    let v = validate(&cfg);
    assert!(!v.ok);
    assert!(v.errors.iter().any(|e| e.contains("tag")));
}

#[test]
fn per_listener_acls_replace_loads() {
    let mut file = NamedTempFile::new().unwrap();
    writeln!(file, "192.0.2.0/24").unwrap();
    let path = file.path().to_str().unwrap();
    let yaml = format!(
        r#"
schema_version: 1
listeners:
  listeners:
    - address: "127.0.0.1:15353"
      protocol: udp
      name: public
      acls:
        default_action: deny
        rules:
          - match: allow_nets
            action: accept
pools:
  - name: primary
    backends:
      - address: "127.0.0.1:5300"
data_sources:
  - name: allow_nets
    type: cidr
    path: {path}
acls:
  default_action: allow
  rules: []
"#
    );
    let cfg = load_yaml(&yaml).unwrap();
    let v = validate(&cfg);
    assert!(v.ok, "{:?}", v.errors);
    let ln = &cfg.listeners.as_ref().unwrap().listeners[0];
    let la = ln.acls.as_ref().unwrap();
    assert_eq!(la.default_action, "deny");
    assert_eq!(la.rules[0].action, "accept");
}
