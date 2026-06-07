//! Config contract: load → validate → export → load (spec §5.1, §5.4).

use conduit_config::{effective_backend_weight, export_yaml, load_yaml, validate};

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
