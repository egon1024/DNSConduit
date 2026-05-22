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
