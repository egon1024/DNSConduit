//! Integration tests for metrics hot-swap (Gate G4 tasks 5.6–5.8).
//!
//! These tests verify:
//! - 5.6: Counter continuity across plan swaps
//! - 5.7: Histogram continuity + schema changes
//! - 5.8: Removed series absent from scrape immediately

use conduit_config::load_yaml;
use conduit_metrics::{compile_from_config, render_prometheus, MetricsHub};
use std::sync::Arc;

fn counter_value(families: &[prometheus::proto::MetricFamily], name: &str) -> u64 {
    families
        .iter()
        .find(|f| f.get_name() == name)
        .and_then(|f| f.get_metric().first())
        .map(|m| m.get_counter().get_value() as u64)
        .unwrap_or(0)
}

fn histogram_count(families: &[prometheus::proto::MetricFamily], name: &str) -> u64 {
    families
        .iter()
        .find(|f| f.get_name() == name)
        .and_then(|f| f.get_metric().first())
        .map(|m| m.get_histogram().get_sample_count())
        .unwrap_or(0)
}

/// Base config YAML with required fields for metrics tests.
fn base_config(profile: &str) -> String {
    format!(
        r#"
schema_version: 1
metrics:
  enabled: true
  profile: {profile}
  prometheus:
    listen_address: "127.0.0.1:9090"
pools:
  - name: default
    backends:
      - address: "127.0.0.1:5300"
listeners:
  listeners:
    - address: "127.0.0.1:15353"
      protocol: udp
"#
    )
}

fn config_with_timing(emit: bool) -> String {
    format!(
        r#"
schema_version: 1
metrics:
  enabled: true
  profile: full
  collection:
    timing:
      collect: true
      emit: {emit}
  prometheus:
    listen_address: "127.0.0.1:9090"
pools:
  - name: default
    backends:
      - address: "127.0.0.1:5300"
listeners:
  listeners:
    - address: "127.0.0.1:15353"
      protocol: udp
"#
    )
}

/// 5.6: Counter continuity — incrementing a counter before and after plan swap
/// should accumulate values (not reset).
#[test]
fn counter_continuity_across_plan_swap() {
    let yaml = base_config("full");
    let cfg = load_yaml(&yaml).unwrap();
    let hub = Arc::new(MetricsHub::from_config(&cfg));

    // Record some queries
    hub.builtin().record_query(
        "127.0.0.1:15353",
        "udp",
        Some(1), // A
        Some(1), // IN
        &"127.0.0.1:12345".parse().unwrap(),
    );
    hub.builtin().record_query(
        "127.0.0.1:15353",
        "udp",
        Some(1),
        Some(1),
        &"127.0.0.1:12346".parse().unwrap(),
    );

    let before = counter_value(&hub.builtin().gather(), "conduit_queries_total");
    assert_eq!(before, 2, "should have 2 queries before swap");

    // Swap to a new plan (same schema, just re-apply)
    let (compiled, _) = compile_from_config(&cfg);
    hub.apply_compiled(compiled);

    // Record more queries after swap
    hub.builtin().record_query(
        "127.0.0.1:15353",
        "udp",
        Some(1),
        Some(1),
        &"127.0.0.1:12347".parse().unwrap(),
    );

    let after = counter_value(&hub.builtin().gather(), "conduit_queries_total");
    assert!(
        after >= before,
        "counter should be >= prior value after swap: before={before}, after={after}"
    );
    assert_eq!(
        after, 3,
        "counter should accumulate across swaps: expected 3, got {after}"
    );
}

/// 5.6 variant: Counter continuity when switching profiles (minimal → full).
#[test]
fn counter_continuity_profile_change() {
    let yaml_minimal = base_config("minimal");
    let cfg_minimal = load_yaml(&yaml_minimal).unwrap();
    let hub = Arc::new(MetricsHub::from_config(&cfg_minimal));

    // Record queries on minimal
    hub.builtin().record_response(
        "127.0.0.1:15353",
        "udp",
        Some(0),
        &"127.0.0.1:12345".parse().unwrap(),
        Some("forward"),
    );

    let before = counter_value(&hub.builtin().gather(), "conduit_responses_total");
    assert!(before >= 1, "should have at least 1 response");

    // Switch to full profile
    let yaml_full = base_config("full");
    let cfg_full = load_yaml(&yaml_full).unwrap();
    let (compiled, _) = compile_from_config(&cfg_full);
    hub.apply_compiled(compiled);

    // Record more after profile change
    hub.builtin().record_response(
        "127.0.0.1:15353",
        "udp",
        Some(0),
        &"127.0.0.1:12346".parse().unwrap(),
        Some("forward"),
    );

    let after = counter_value(&hub.builtin().gather(), "conduit_responses_total");
    assert!(
        after >= before,
        "counter should be >= prior value after profile change"
    );
}

/// 5.7: Histogram continuity — observations should accumulate across swaps.
#[test]
fn histogram_continuity_across_plan_swap() {
    let yaml = base_config("full");
    let cfg = load_yaml(&yaml).unwrap();
    let hub = Arc::new(MetricsHub::from_config(&cfg));

    // Record some forward durations
    hub.builtin()
        .record_forward_duration("default", "127.0.0.1:5300", 0.001);
    hub.builtin()
        .record_forward_duration("default", "127.0.0.1:5300", 0.002);

    let before = histogram_count(&hub.builtin().gather(), "conduit_forward_duration_seconds");
    assert_eq!(before, 2, "should have 2 observations before swap");

    // Swap plan
    let (compiled, _) = compile_from_config(&cfg);
    hub.apply_compiled(compiled);

    // Record more after swap
    hub.builtin()
        .record_forward_duration("default", "127.0.0.1:5300", 0.003);

    let after = histogram_count(&hub.builtin().gather(), "conduit_forward_duration_seconds");
    assert!(
        after >= before,
        "histogram count should be >= prior after swap"
    );
    assert_eq!(
        after, 3,
        "histogram should accumulate: expected 3, got {after}"
    );
}

/// 5.8: Removed series absent from scrape immediately — when timing category
/// is disabled, timing metrics should not appear in scrape output even if
/// the underlying handles still exist.
#[test]
fn removed_series_absent_from_scrape_immediately() {
    // Start with timing enabled
    let yaml_with_timing = config_with_timing(true);
    let cfg = load_yaml(&yaml_with_timing).unwrap();
    let hub = Arc::new(MetricsHub::from_config(&cfg));

    // Record some timing metrics
    hub.builtin()
        .record_forward_attempt("default", "127.0.0.1:5300", "success");
    hub.builtin()
        .record_forward_duration("default", "127.0.0.1:5300", 0.001);

    let body_before = render_prometheus(hub.as_ref(), &[]);
    assert!(
        body_before.contains("conduit_forward_attempts_total"),
        "timing should be in scrape before swap"
    );
    assert!(
        body_before.contains("conduit_forward_duration_seconds"),
        "timing histogram should be in scrape before swap"
    );

    // Swap to config with timing emit disabled
    let yaml_no_timing = config_with_timing(false);
    let cfg_no_timing = load_yaml(&yaml_no_timing).unwrap();
    let (compiled, _) = compile_from_config(&cfg_no_timing);
    hub.apply_compiled(compiled);

    let body_after = render_prometheus(hub.as_ref(), &[]);
    assert!(
        !body_after.contains("conduit_forward_attempts_total"),
        "timing should NOT be in scrape after emit disabled:\n{body_after}"
    );
    assert!(
        !body_after.contains("conduit_forward_duration_seconds"),
        "timing histogram should NOT be in scrape after emit disabled:\n{body_after}"
    );

    // Volume metrics should still be present
    hub.builtin().record_query(
        "127.0.0.1:15353",
        "udp",
        Some(1),
        Some(1),
        &"127.0.0.1:12345".parse().unwrap(),
    );
    let body_final = render_prometheus(hub.as_ref(), &[]);
    assert!(
        body_final.contains("conduit_queries_total"),
        "volume metrics should still emit:\n{body_final}"
    );
}

/// 5.8 variant: Disabled metrics category does not appear in scrape.
#[test]
fn disabled_category_absent_from_scrape() {
    // Start with volume enabled, failures disabled
    let yaml = r#"
schema_version: 1
metrics:
  enabled: true
  profile: full
  collection:
    volume:
      collect: true
      emit: true
    failures:
      collect: false
      emit: false
  prometheus:
    listen_address: "127.0.0.1:9090"
pools:
  - name: default
    backends:
      - address: "127.0.0.1:5300"
listeners:
  listeners:
    - address: "127.0.0.1:15353"
      protocol: udp
"#;
    let cfg = load_yaml(yaml).unwrap();
    let hub = Arc::new(MetricsHub::from_config(&cfg));

    // Record metrics
    hub.builtin().record_query(
        "127.0.0.1:15353",
        "udp",
        Some(1),
        Some(1),
        &"127.0.0.1:12345".parse().unwrap(),
    );
    hub.builtin().record_parse_rejected("empty");

    let body = render_prometheus(hub.as_ref(), &[]);
    assert!(
        body.contains("conduit_queries_total"),
        "volume should be present"
    );
    // parse_rejected is in failures category
    assert!(
        !body.contains("conduit_parse_rejected_total"),
        "failures category should not emit:\n{body}"
    );
}

/// Generation tracking works correctly across swaps.
#[test]
fn generation_increments_on_swap() {
    let yaml = base_config("full");
    let cfg = load_yaml(&yaml).unwrap();
    let hub = Arc::new(MetricsHub::from_config(&cfg));

    let gen0 = hub.generation();

    let (compiled, _) = compile_from_config(&cfg);
    hub.apply_compiled(compiled);
    let gen1 = hub.generation();

    let (compiled2, _) = compile_from_config(&cfg);
    hub.apply_compiled(compiled2);
    let gen2 = hub.generation();

    assert!(gen1 > gen0, "generation should increment after first swap");
    assert!(gen2 > gen1, "generation should increment after second swap");
}
