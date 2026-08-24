//! Config contract: load → validate → export → load (spec §5.1, §5.4).

use conduit_config::{effective_backend_weight, export_yaml, load_yaml, validate};

/// Wire-enum YAML selectors (`qtype`, `qclass`, `opcode`, `edns_option`) load and
/// validate using the same IANA names / numeric aliases as Rule Rhai.
#[test]
fn wire_enum_selectors_fixture_validates() {
    let yaml = include_str!("../../../tests/fixtures/config/with-rules-wire-enum-selectors.yaml");
    let cfg = load_yaml(yaml).expect("load wire-enum selector fixture");
    let validation = validate(&cfg);
    assert!(validation.ok, "validation failed: {:?}", validation.errors);

    // Round-trips through export without losing the selectors.
    let out = export_yaml(&cfg).expect("export");
    let cfg2 = load_yaml(&out).expect("reload exported yaml");
    let validation2 = validate(&cfg2);
    assert!(
        validation2.ok,
        "reload validation: {:?}",
        validation2.errors
    );
}

/// The `data_source_limits` block and per-entry overrides load, validate, and
/// round-trip through export without loss.
#[test]
fn data_source_limits_fixture_round_trips() {
    let yaml = include_str!("../../../tests/fixtures/config/with-data-source-limits.yaml");
    let cfg = load_yaml(yaml).expect("load data-source-limits fixture");
    let validation = validate(&cfg);
    assert!(validation.ok, "validation failed: {:?}", validation.errors);

    let limits = cfg
        .data_source_limits
        .as_ref()
        .expect("data_source_limits present");
    assert_eq!(limits.max_file_bytes, 1_048_576);
    assert_eq!(limits.max_entries, 50_000);
    assert_eq!(limits.max_tables, 8);
    assert_eq!(cfg.data_sources[0].max_file_bytes, Some(65_536));
    assert_eq!(cfg.data_sources[0].max_entries, Some(1_000));

    let out = export_yaml(&cfg).expect("export");
    let cfg2 = load_yaml(&out).expect("reload exported yaml");
    let validation2 = validate(&cfg2);
    assert!(
        validation2.ok,
        "reload validation: {:?}",
        validation2.errors
    );
    let limits2 = cfg2
        .data_source_limits
        .as_ref()
        .expect("limits survive round-trip");
    assert_eq!(limits2.max_entries, 50_000);
    assert_eq!(cfg2.data_sources[0].max_entries, Some(1_000));
}

/// `data_source_limits.max_tables` smaller than the number of entries is rejected
/// structurally at validation (before any file read).
#[test]
fn max_tables_smaller_than_entry_count_is_rejected() {
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
        weight: 100
data_source_limits:
  max_tables: 1
data_sources:
  - name: a
    type: csv
    path: ../data/blocklist.csv
  - name: b
    type: csv
    path: ../data/blocklist.csv
"#;
    let cfg = load_yaml(yaml).expect("load config");
    let validation = validate(&cfg);
    assert!(!validation.ok, "expected max_tables rejection");
    assert!(
        validation.errors.iter().any(|e| e.contains("max_tables")),
        "errors: {:?}",
        validation.errors
    );
}

/// An unknown `qtype` selector value is rejected at config validation.
#[test]
fn unknown_qtype_selector_value_is_rejected() {
    let yaml = r#"
schema_version: 1
listeners:
  threads: 1
  listeners:
    - address: "127.0.0.1:15353"
      protocol: udp
pools:
  - name: primary
    backends:
      - address: "127.0.0.1:5300"
        weight: 100
rules:
  match_mode: first_match
  rules:
    - name: bogus-qtype
      hook: request
      selectors:
        - type: qtype
          value: NOT_A_REAL_TYPE
      actions:
        - type: set_pool
          value: primary
"#;
    let cfg = load_yaml(yaml).expect("load config with bogus qtype");
    let validation = validate(&cfg);
    assert!(
        !validation.ok,
        "expected validation to reject unknown qtype"
    );
    assert!(
        validation
            .errors
            .iter()
            .any(|e| e.contains("NOT_A_REAL_TYPE")),
        "error should name the offending value: {:?}",
        validation.errors
    );
}

/// set_pool and set_retry_pool must name a pool declared in pools:.
#[test]
fn unknown_pool_in_set_pool_is_rejected() {
    let yaml = r#"
schema_version: 1
listeners:
  threads: 1
  listeners:
    - address: "127.0.0.1:15353"
      protocol: udp
pools:
  - name: primary
    backends:
      - address: "127.0.0.1:5300"
        weight: 100
rules:
  match_mode: first_match
  rules:
    - name: bad-pool
      hook: request
      selectors: []
      actions:
        - type: set_pool
          value: does-not-exist
"#;
    let cfg = load_yaml(yaml).expect("load");
    let validation = validate(&cfg);
    assert!(!validation.ok, "expected unknown pool rejection");
    assert!(
        validation.errors.iter().any(|e| {
            e.contains("bad-pool") && e.contains("set_pool") && e.contains("does-not-exist")
        }),
        "errors: {:?}",
        validation.errors
    );
}

#[test]
fn unknown_pool_in_set_retry_pool_is_rejected() {
    let yaml = r#"
schema_version: 1
listeners:
  threads: 1
  listeners:
    - address: "127.0.0.1:15353"
      protocol: udp
pools:
  - name: primary
    backends:
      - address: "127.0.0.1:5300"
        weight: 100
rules:
  match_mode: first_match
  rules:
    - name: bad-retry-pool
      hook: response
      selectors:
        - type: rcode
          value: SERVFAIL
      actions:
        - type: set_retry_pool
          value: backup
        - type: retry
"#;
    let cfg = load_yaml(yaml).expect("load");
    let validation = validate(&cfg);
    assert!(!validation.ok);
    assert!(
        validation.errors.iter().any(|e| {
            e.contains("bad-retry-pool") && e.contains("set_retry_pool") && e.contains("backup")
        }),
        "errors: {:?}",
        validation.errors
    );
}

#[test]
fn set_pool_to_declared_pool_validates() {
    let yaml = r#"
schema_version: 1
listeners:
  threads: 1
  listeners:
    - address: "127.0.0.1:15353"
      protocol: udp
pools:
  - name: primary
    backends:
      - address: "127.0.0.1:5300"
        weight: 100
rules:
  match_mode: first_match
  rules:
    - name: ok-pool
      hook: request
      selectors: []
      actions:
        - type: set_pool
          value: primary
"#;
    let cfg = load_yaml(yaml).expect("load");
    let validation = validate(&cfg);
    assert!(validation.ok, "errors: {:?}", validation.errors);
}

/// A `qtype` IANA name and its `TYPE{n}` numeric alias compile to the same selector,
/// so both forms validate identically.
#[test]
fn qtype_name_and_numeric_alias_both_validate() {
    let make = |value: &str| {
        format!(
            r#"
schema_version: 1
listeners:
  threads: 1
  listeners:
    - address: "127.0.0.1:15353"
      protocol: udp
pools:
  - name: primary
    backends:
      - address: "127.0.0.1:5300"
        weight: 100
rules:
  match_mode: first_match
  rules:
    - name: alias-equiv
      hook: request
      selectors:
        - type: qtype
          value: {value}
      actions:
        - type: set_pool
          value: primary
"#
        )
    };
    for value in ["A", "TYPE1"] {
        let cfg = load_yaml(&make(value)).expect("load");
        let validation = validate(&cfg);
        assert!(
            validation.ok,
            "qtype {value} should validate: {:?}",
            validation.errors
        );
    }
}

#[test]
fn rules_unknown_set_pool_fixture_rejected() {
    let yaml = include_str!("../../../tests/fixtures/config/rules-unknown-set-pool.yml");
    let cfg = load_yaml(yaml).expect("load fixture");
    let validation = validate(&cfg);
    assert!(!validation.ok);
    assert!(
        validation
            .errors
            .iter()
            .any(|e| { e.contains("typo-pool") && e.contains("set_pool") && e.contains("primry") }),
        "errors: {:?}",
        validation.errors
    );
}

#[test]
fn config_contract_roundtrip() {
    let yaml = include_str!("../../../tests/fixtures/config/minimal.yaml");
    let cfg = load_yaml(yaml).expect("load fixture");
    let validation = validate(&cfg);
    assert!(validation.ok, "validation failed: {:?}", validation.errors);

    let out = export_yaml(&cfg).expect("export");
    let cfg2 = load_yaml(&out).expect("reload exported yaml");

    assert_eq!(cfg.schema_version, cfg2.schema_version);
    assert_eq!(
        cfg.listeners.as_ref().unwrap().threads,
        cfg2.listeners.as_ref().unwrap().threads
    );
    assert_eq!(
        effective_backend_weight(&cfg.pools[0].backends[0]),
        effective_backend_weight(&cfg2.pools[0].backends[0])
    );
}

#[test]
fn sparse_config_contract_roundtrip() {
    let yaml = include_str!("../../../tests/fixtures/config/minimal-sparse.yaml");
    let cfg = load_yaml(yaml).expect("load sparse fixture");
    let validation = validate(&cfg);
    assert!(validation.ok, "validation failed: {:?}", validation.errors);
    assert!(cfg.control.is_none());

    let out = export_yaml(&cfg).expect("export");
    assert!(!out.contains("control:"));
    let cfg2 = load_yaml(&out).expect("reload exported yaml");

    assert_eq!(cfg.schema_version, cfg2.schema_version);
    assert_eq!(cfg.forward, cfg2.forward);
    assert_eq!(cfg.orchestrator, cfg2.orchestrator);
    assert_eq!(cfg.events, cfg2.events);
    assert_eq!(cfg.rhai, cfg2.rhai);
    assert!(cfg2.control.is_none());
    assert_eq!(
        cfg.listeners.as_ref().unwrap().listeners,
        cfg2.listeners.as_ref().unwrap().listeners
    );
    assert_eq!(cfg.pools[0].name, cfg2.pools[0].name);
}
