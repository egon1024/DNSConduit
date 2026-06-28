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
