//! Integration tests for metrics export hot-rebind (Gate G4 tasks 5.9–5.10).
//!
//! These tests verify:
//! - 5.9: Prometheus listen_address/path rebind without restart; bind failure rejects apply
//! - 5.10: OTLP endpoint change reconnects; in-place update for interval/headers

use conduit_config::load_yaml;
use conduit_events::EventHub;
use conduit_metrics::{compile_from_config, MetricsExportController, MetricsHub};
use std::sync::Arc;
use tokio::net::TcpListener;

/// Create a disabled EventHub for tests (no event sinks needed).
fn test_events() -> Arc<EventHub> {
    Arc::new(EventHub::disabled())
}

/// Base config YAML with Prometheus on an ephemeral port.
fn config_with_prom(port: u16, path: &str) -> String {
    format!(
        r#"
schema_version: 1
metrics:
  enabled: true
  base: standard
  prometheus:
    listen_address: "127.0.0.1:{port}"
    path: {path}
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

fn config_with_otel(endpoint: &str, interval_ms: u32) -> String {
    format!(
        r#"
schema_version: 1
metrics:
  enabled: true
  base: standard
  otel:
    endpoint: "{endpoint}"
    push_interval_ms: {interval_ms}
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

/// Allocate an ephemeral port and return its number.
async fn ephemeral_port() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    listener.local_addr().unwrap().port()
}

/// 5.9: Prometheus rebind success — changing port pre-binds new, commits, serves on new.
#[tokio::test]
async fn prometheus_rebind_success() {
    let port1 = ephemeral_port().await;
    let port2 = ephemeral_port().await;

    let yaml1 = config_with_prom(port1, "/metrics");
    let cfg1 = load_yaml(&yaml1).unwrap();
    let hub = Arc::new(MetricsHub::from_config(&cfg1));
    let events = test_events();

    let controller = Arc::new(MetricsExportController::new(hub.clone()));
    let (compiled1, _) = compile_from_config(&cfg1);
    controller.initial_spawn(&compiled1, events.clone()).await;

    // Verify initial port is serving
    let resp1 = reqwest::get(format!("http://127.0.0.1:{}/metrics", port1)).await;
    assert!(resp1.is_ok(), "initial port should be serving");

    // Prepare rebind to new port
    let yaml2 = config_with_prom(port2, "/metrics");
    let cfg2 = load_yaml(&yaml2).unwrap();
    let (compiled2, _) = compile_from_config(&cfg2);

    let pending = controller.prepare(&compiled2).await;
    assert!(pending.is_ok(), "prepare should succeed for valid port");

    // Commit the rebind
    controller.commit(pending.unwrap(), events.clone()).await;

    // Verify new port is serving
    let resp2 = reqwest::get(format!("http://127.0.0.1:{}/metrics", port2)).await;
    assert!(resp2.is_ok(), "new port should be serving after rebind");

    // Old port should no longer serve
    let resp1_after = reqwest::get(format!("http://127.0.0.1:{}/metrics", port1))
        .await
        .ok()
        .and_then(|r| {
            if r.status().is_success() {
                Some(r)
            } else {
                None
            }
        });
    assert!(
        resp1_after.is_none(),
        "old port should not be serving after rebind"
    );

    controller.shutdown().await;
}

/// 5.9: Prometheus bind failure rejects prepare, keeps last-good.
#[tokio::test]
async fn prometheus_bind_failure_rejects_prepare() {
    let port1 = ephemeral_port().await;

    let yaml1 = config_with_prom(port1, "/metrics");
    let cfg1 = load_yaml(&yaml1).unwrap();
    let hub = Arc::new(MetricsHub::from_config(&cfg1));
    let events = test_events();

    let controller = Arc::new(MetricsExportController::new(hub.clone()));
    let (compiled1, _) = compile_from_config(&cfg1);
    controller.initial_spawn(&compiled1, events.clone()).await;

    // Occupy a second port to cause bind failure
    let blocker = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let occupied_port = blocker.local_addr().unwrap().port();

    // Try to rebind to the occupied port
    let yaml2 = config_with_prom(occupied_port, "/metrics");
    let cfg2 = load_yaml(&yaml2).unwrap();
    let (compiled2, _) = compile_from_config(&cfg2);

    let pending = controller.prepare(&compiled2).await;
    assert!(pending.is_err(), "prepare should fail for occupied port");
    let err = pending.unwrap_err();
    assert!(
        err.contains("rebind") && err.contains("failed"),
        "error should mention rebind failure: {}",
        err
    );

    // Verify original port is still serving (last-good preserved)
    let resp1 = reqwest::get(format!("http://127.0.0.1:{}/metrics", port1)).await;
    assert!(
        resp1.is_ok(),
        "original port should still be serving after bind failure"
    );

    controller.shutdown().await;
}

/// 5.9: Plan-only change (no address/path change) does not rebind.
#[tokio::test]
async fn plan_only_change_no_rebind() {
    let port = ephemeral_port().await;

    // Start with base: standard
    let yaml1 = format!(
        r#"
schema_version: 1
metrics:
  enabled: true
  base: standard
  prometheus:
    listen_address: "127.0.0.1:{port}"
    path: /metrics
pools:
  - name: default
    backends:
      - address: "127.0.0.1:5300"
listeners:
  listeners:
    - address: "127.0.0.1:15353"
      protocol: udp
"#
    );

    let cfg1 = load_yaml(&yaml1).unwrap();
    let hub = Arc::new(MetricsHub::from_config(&cfg1));
    let events = test_events();

    let controller = Arc::new(MetricsExportController::new(hub.clone()));
    let (compiled1, _) = compile_from_config(&cfg1);
    controller.initial_spawn(&compiled1, events.clone()).await;

    // Change only collection (plan-only), same address/path
    let yaml2 = format!(
        r#"
schema_version: 1
metrics:
  enabled: true
  base: standard
  collection:
    timing:
      collect: true
      emit: false
  prometheus:
    listen_address: "127.0.0.1:{port}"
    path: /metrics
pools:
  - name: default
    backends:
      - address: "127.0.0.1:5300"
listeners:
  listeners:
    - address: "127.0.0.1:15353"
      protocol: udp
"#
    );

    let cfg2 = load_yaml(&yaml2).unwrap();
    let (compiled2, _) = compile_from_config(&cfg2);

    // Verify controller detects plan-only change
    assert!(
        controller.is_plan_only_change(&compiled2),
        "changing only collection should be plan-only"
    );

    // Prepare should succeed with no rebind
    let pending = controller.prepare(&compiled2).await;
    assert!(pending.is_ok());

    // Port should still be serving
    let resp = reqwest::get(format!("http://127.0.0.1:{}/metrics", port)).await;
    assert!(
        resp.is_ok(),
        "port should continue serving during plan-only change"
    );

    controller.shutdown().await;
}

/// 5.10: OTLP endpoint change triggers reconnect.
#[tokio::test]
async fn otel_endpoint_change_triggers_reconnect() {
    let yaml1 = config_with_otel("http://127.0.0.1:4318/v1/metrics", 15000);
    let cfg1 = load_yaml(&yaml1).unwrap();
    let hub = Arc::new(MetricsHub::from_config(&cfg1));
    let events = test_events();

    let controller = Arc::new(MetricsExportController::new(hub.clone()));
    let (compiled1, _) = compile_from_config(&cfg1);
    controller.initial_spawn(&compiled1, events.clone()).await;

    // Change endpoint
    let yaml2 = config_with_otel("http://127.0.0.1:4319/v1/metrics", 15000);
    let cfg2 = load_yaml(&yaml2).unwrap();
    let (compiled2, _) = compile_from_config(&cfg2);

    // Should not be plan-only (endpoint changed)
    assert!(
        !controller.is_plan_only_change(&compiled2),
        "endpoint change should not be plan-only"
    );

    let pending = controller.prepare(&compiled2).await;
    assert!(pending.is_ok());
    controller.commit(pending.unwrap(), events.clone()).await;

    controller.shutdown().await;
}

/// 5.10: OTLP interval-only change is in-place (no reconnect key change).
#[tokio::test]
async fn otel_interval_only_is_inplace() {
    let yaml1 = config_with_otel("http://127.0.0.1:4318/v1/metrics", 15000);
    let cfg1 = load_yaml(&yaml1).unwrap();
    let hub = Arc::new(MetricsHub::from_config(&cfg1));
    let events = test_events();

    let controller = Arc::new(MetricsExportController::new(hub.clone()));
    let (compiled1, _) = compile_from_config(&cfg1);
    controller.initial_spawn(&compiled1, events.clone()).await;

    // Change only interval (in-place update, not reconnect)
    let yaml2 = config_with_otel("http://127.0.0.1:4318/v1/metrics", 30000);
    let cfg2 = load_yaml(&yaml2).unwrap();
    let (compiled2, _) = compile_from_config(&cfg2);

    // Should be plan-only for Prometheus (none configured), but OTLP has in-place change
    let pending = controller.prepare(&compiled2).await;
    assert!(pending.is_ok());
    let pending = pending.unwrap();

    // OtelChange should be InPlace, not Reconnect
    assert_eq!(
        pending.otel_change,
        conduit_metrics::OtelChange::InPlace,
        "interval-only change should be InPlace"
    );

    controller.commit(pending, events.clone()).await;
    controller.shutdown().await;
}

/// 5.9: Path change triggers rebind (even with same port).
#[tokio::test]
async fn prometheus_path_change_triggers_rebind() {
    let port = ephemeral_port().await;

    let yaml1 = config_with_prom(port, "/metrics");
    let cfg1 = load_yaml(&yaml1).unwrap();
    let hub = Arc::new(MetricsHub::from_config(&cfg1));
    let events = test_events();

    let controller = Arc::new(MetricsExportController::new(hub.clone()));
    let (compiled1, _) = compile_from_config(&cfg1);
    controller.initial_spawn(&compiled1, events.clone()).await;

    // Change path only
    let yaml2 = config_with_prom(port, "/custom");
    let cfg2 = load_yaml(&yaml2).unwrap();
    let (compiled2, _) = compile_from_config(&cfg2);

    // Should not be plan-only (path changed)
    assert!(
        !controller.is_plan_only_change(&compiled2),
        "path change should trigger rebind"
    );

    let pending = controller.prepare(&compiled2).await;
    assert!(pending.is_ok());
    controller.commit(pending.unwrap(), events.clone()).await;

    // Verify new path is serving
    let resp = reqwest::get(format!("http://127.0.0.1:{}/custom", port)).await;
    assert!(resp.is_ok(), "new path should be serving after rebind");

    // Old path should 404
    let resp_old = reqwest::get(format!("http://127.0.0.1:{}/metrics", port)).await;
    assert!(
        resp_old.is_ok() && resp_old.unwrap().status() == 404,
        "old path should return 404 after rebind"
    );

    controller.shutdown().await;
}
