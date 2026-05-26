//! Phase 1b slice A: forward sources_v4, compiled snapshot, and rd.rs wire helper.

use conduit_config::forward::RecursionDesired;
use conduit_config::{load_yaml, validate};
use conduit_core::snapshot::RuntimeSnapshot;
use conduit_dataplane::forward::rd::build_upstream_wire;
use hickory_proto::op::{Message, Query};
use hickory_proto::rr::{Name, RecordType};
use hickory_proto::serialize::binary::{BinEncodable, BinEncoder};

fn sample_query(rd: bool) -> Vec<u8> {
    let name = Name::from_utf8("test.example.").unwrap();
    let query = Query::query(name, RecordType::A);
    let mut msg = Message::new();
    let mut header = *msg.header();
    header.set_id(0xabcd);
    header.set_recursion_desired(rd);
    msg.set_header(header);
    msg.add_query(query);
    let mut out = Vec::new();
    let mut enc = BinEncoder::new(&mut out);
    msg.emit(&mut enc).unwrap();
    out
}

#[test]
fn forward_sources_v4_fixture_compiles() {
    let yaml = include_str!("../../../tests/fixtures/config/forward-sources-v4.yaml");
    let cfg = load_yaml(yaml).unwrap();
    assert!(validate(&cfg).ok);
    let snap = RuntimeSnapshot::from_config(cfg);
    assert_eq!(snap.forward.sources_v4.len(), 1);
}

/// Unit test for `forward::rd::build_upstream_wire` (not YAML RD policy; Rhai-only at runtime).
#[test]
fn build_upstream_wire_clear_zeros_rd() {
    let q = sample_query(true);
    let out = build_upstream_wire(&q, RecursionDesired::Clear);
    assert!(!Message::from_vec(&out)
        .unwrap()
        .header()
        .recursion_desired());
}

#[test]
fn forward_sources_v6_fixture_compiles() {
    let yaml = include_str!("../../../tests/fixtures/config/forward-sources-v6.yaml");
    let cfg = load_yaml(yaml).unwrap();
    assert!(validate(&cfg).ok);
    let snap = RuntimeSnapshot::from_config(cfg);
    assert_eq!(snap.forward.sources_v6.len(), 1);
}

#[test]
fn minimal_config_default_forward_unchanged() {
    let yaml = include_str!("../../../tests/fixtures/config/dataplane-minimal.yaml");
    let cfg = load_yaml(yaml).unwrap();
    let snap = RuntimeSnapshot::from_config(cfg);
    assert!(snap.forward.sources_v4.is_empty());
}
