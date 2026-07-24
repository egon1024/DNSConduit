//! Hot-rebind controller for Prometheus and OTLP metric export sinks.
//!
//! Implements the prepare/commit pattern for metrics export sink changes:
//!
//! 1. **Prepare** (before snapshot install): attempt to bind/reconnect new sinks.
//!    If bind fails, return `Err` to reject the apply before any snapshot change.
//!
//! 2. **Commit** (after successful snapshot install + `apply_compiled`): activate
//!    new sinks and shut down old ones.
//!
//! This guarantees that a bind failure keeps the last-good listener alive and
//! does not mutate the running config.
//!
//! Plan-only changes (categories, collect/emit, granularity) that do not alter
//! sink targets (`listen_address`, `path`, `endpoint`, TLS settings) skip rebind
//! entirely—the same socket/client continues serving.

use crate::compile::CompiledMetrics;
use crate::otel::{build_otel_push_loop, OtelPushSettings};
use crate::prometheus_http::PrometheusServer;
use crate::task::{OtelPushHandle, PrometheusServerHandle};
use crate::MetricsHub;
use arc_swap::ArcSwap;
use conduit_events::EventHub;
use parking_lot::Mutex;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::TcpListener;

/// Identifies which export sink parameters changed (used to decide rebind vs in-place update).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrometheusChange {
    /// No change to listen_address or path—plan-only update.
    None,
    /// listen_address changed—requires pre-bind then rebind.
    AddressChange,
    /// Only path changed (same address)—requires restart but no pre-bind.
    PathChange,
    /// Prometheus being disabled.
    Disable,
}

/// Identifies which OTLP parameters changed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OtelChange {
    /// No OTLP config present or no change.
    None,
    /// Only interval/headers/resource_attributes changed—in-place update.
    InPlace,
    /// Endpoint or TLS changed—requires client rebuild.
    Reconnect,
    /// OTLP was enabled (from disabled) or disabled (from enabled).
    Toggle,
}

/// Keys that determine whether Prometheus needs a rebind.
#[derive(Debug, Clone, PartialEq, Eq)]
struct PrometheusBindKey {
    listen_address: Option<String>,
    path: String,
}

impl PrometheusBindKey {
    fn from_compiled(c: &CompiledMetrics) -> Self {
        Self {
            listen_address: c.prometheus_listen.clone(),
            path: c.prometheus_path.clone(),
        }
    }
}

/// Keys that determine whether OTLP needs a reconnect vs in-place update.
#[derive(Debug, Clone, PartialEq, Eq)]
struct OtelReconnectKey {
    endpoint: Option<String>,
    allow_invalid_certs: bool,
}

impl OtelReconnectKey {
    fn from_compiled(c: &CompiledMetrics) -> Self {
        Self {
            endpoint: c.otel_endpoint.clone(),
            allow_invalid_certs: c.otel_allow_invalid_certs,
        }
    }
}

/// In-place updatable OTLP settings (can change without reconnect).
#[derive(Debug, Clone, PartialEq, Eq)]
struct OtelInPlaceKey {
    push_interval_ms: u32,
    headers: Vec<(String, String)>,
    resource_attributes: Vec<(String, String)>,
}

impl OtelInPlaceKey {
    fn from_compiled(c: &CompiledMetrics) -> Self {
        Self {
            push_interval_ms: c.otel_push_interval_ms,
            headers: c.otel_headers.clone(),
            resource_attributes: c.otel_resource_attributes.clone(),
        }
    }
}

/// Result of a successful `prepare()` call. Passed to `commit()` to finalize changes.
#[derive(Debug)]
pub struct PendingExportChange {
    /// Pre-bound TCP listener for Prometheus (None if no rebind needed or disabled).
    pub(crate) new_prometheus_listener: Option<TcpListener>,
    pub(crate) new_prometheus_path: Option<String>,
    /// What kind of Prometheus change is pending.
    pub prometheus_change: PrometheusChange,

    /// New OTLP settings (None if no change or disabled).
    pub(crate) new_otel_settings: Option<OtelPushSettings>,
    /// What kind of OTLP change is pending.
    pub otel_change: OtelChange,

    /// Snapshot of compiled metrics for the new config.
    pub(crate) compiled: CompiledMetrics,
}

/// Hot-swappable settings that the OTLP push loop reads each iteration.
///
/// Held in an `ArcSwap` so the push loop can read updated interval/headers/attrs
/// without rebuilding the HTTP client.
#[derive(Debug, Clone)]
pub struct OtelHotSettings {
    pub push_interval_ms: u32,
    pub headers: Vec<(String, String)>,
    pub resource_attributes: Vec<(String, String)>,
}

/// Controller for Prometheus and OTLP export sinks.
///
/// Shared between `RuntimeSupervisor` (initial spawn) and `Configurator` (hot apply).
/// Thread-safe via internal `Mutex` and `ArcSwap` for hot settings.
pub struct MetricsExportController {
    /// Current Prometheus server handle (None if disabled).
    prometheus: Mutex<Option<PrometheusServerHandle>>,
    /// Current snapshot of Prometheus bind key (for change detection).
    prometheus_key: Mutex<PrometheusBindKey>,

    /// Current OTLP push handle (None if disabled).
    otel: Mutex<Option<OtelPushHandle>>,
    /// Current snapshot of OTLP reconnect key (for change detection).
    otel_reconnect_key: Mutex<OtelReconnectKey>,
    /// Hot-swappable OTLP settings (interval/headers/attrs).
    otel_hot_settings: Arc<ArcSwap<OtelHotSettings>>,

    /// Metrics hub reference (for spawning new servers).
    hub: Arc<MetricsHub>,
}

impl MetricsExportController {
    /// Create a new controller without any active sinks.
    ///
    /// Call `initial_spawn` after construction to start sinks based on initial config.
    pub fn new(hub: Arc<MetricsHub>) -> Self {
        Self {
            prometheus: Mutex::new(None),
            prometheus_key: Mutex::new(PrometheusBindKey {
                listen_address: None,
                path: "/metrics".into(),
            }),
            otel: Mutex::new(None),
            otel_reconnect_key: Mutex::new(OtelReconnectKey {
                endpoint: None,
                allow_invalid_certs: false,
            }),
            otel_hot_settings: Arc::new(ArcSwap::from_pointee(OtelHotSettings {
                push_interval_ms: 15_000,
                headers: vec![],
                resource_attributes: vec![],
            })),
            hub,
        }
    }

    /// Spawn initial sinks based on compiled config.
    ///
    /// Called once at process start. Unlike `prepare`/`commit`, this binds
    /// synchronously and logs on failure (does not fail the process).
    pub async fn initial_spawn(&self, compiled: &CompiledMetrics, events: Arc<EventHub>) {
        // Prometheus
        if let Some(ref addr_str) = compiled.prometheus_listen {
            if let Ok(addr) = addr_str.parse::<SocketAddr>() {
                match TcpListener::bind(addr).await {
                    Ok(listener) => {
                        let handle = PrometheusServer::spawn_with_listener(
                            listener,
                            compiled.prometheus_path.clone(),
                            self.hub.clone(),
                            events.clone(),
                        );
                        *self.prometheus.lock() = Some(handle);
                        *self.prometheus_key.lock() = PrometheusBindKey::from_compiled(compiled);
                        tracing::info!(%addr, path = %compiled.prometheus_path, "prometheus metrics listening");
                    }
                    Err(e) => {
                        tracing::error!(%addr, error = %e, "prometheus metrics bind failed at startup");
                    }
                }
            }
        }

        // OTLP
        if let Some(ref endpoint) = compiled.otel_endpoint {
            let settings = OtelPushSettings {
                endpoint: endpoint.clone(),
                push_interval_ms: compiled.otel_push_interval_ms,
                resource_attributes: compiled.otel_resource_attributes.clone(),
                allow_invalid_certs: compiled.otel_allow_invalid_certs,
                headers: compiled.otel_headers.clone(),
            };
            let hot_settings = OtelHotSettings {
                push_interval_ms: compiled.otel_push_interval_ms,
                headers: compiled.otel_headers.clone(),
                resource_attributes: compiled.otel_resource_attributes.clone(),
            };
            self.otel_hot_settings.store(Arc::new(hot_settings));

            let handle = build_otel_push_loop(
                settings,
                self.hub.clone(),
                events,
                self.otel_hot_settings.clone(),
            );
            *self.otel.lock() = Some(handle);
            *self.otel_reconnect_key.lock() = OtelReconnectKey::from_compiled(compiled);
            tracing::info!(%endpoint, "otel metrics push started");
        }
    }

    /// Prepare for a config change by pre-binding new resources if needed.
    ///
    /// Returns `Ok(PendingExportChange)` on success—call `commit()` after
    /// `install_validated_with_base` and `apply_compiled` succeed.
    ///
    /// Returns `Err(message)` if bind fails—reject the apply without installing
    /// the snapshot.
    pub async fn prepare(&self, new: &CompiledMetrics) -> Result<PendingExportChange, String> {
        let current_prom_key = self.prometheus_key.lock().clone();
        let new_prom_key = PrometheusBindKey::from_compiled(new);

        // Determine what kind of Prometheus change this is
        let prometheus_change = if current_prom_key == new_prom_key {
            PrometheusChange::None
        } else if new_prom_key.listen_address.is_none() {
            // Prometheus being disabled
            PrometheusChange::Disable
        } else if current_prom_key.listen_address != new_prom_key.listen_address {
            // Address changed—requires pre-bind
            PrometheusChange::AddressChange
        } else {
            // Only path changed (same address)—restart without pre-bind
            PrometheusChange::PathChange
        };

        // Pre-bind new Prometheus listener only if address changed
        let (new_prometheus_listener, new_prometheus_path) = match prometheus_change {
            PrometheusChange::AddressChange => {
                let addr_str = new.prometheus_listen.as_ref().unwrap();
                let addr: SocketAddr = addr_str.parse().map_err(|e| {
                    format!("invalid prometheus listen_address '{}': {}", addr_str, e)
                })?;
                let listener = TcpListener::bind(addr).await.map_err(|e| {
                    format!(
                        "prometheus rebind to {} failed: {}; keeping last-good listener",
                        addr, e
                    )
                })?;
                tracing::debug!(%addr, "prometheus pre-bound for rebind");
                (Some(listener), Some(new.prometheus_path.clone()))
            }
            PrometheusChange::PathChange => {
                // Path-only change: we'll rebind after stopping the old server
                (None, Some(new.prometheus_path.clone()))
            }
            PrometheusChange::Disable | PrometheusChange::None => (None, None),
        };

        // Determine OTLP change type
        let current_otel_key = self.otel_reconnect_key.lock().clone();
        let new_otel_key = OtelReconnectKey::from_compiled(new);
        let current_otel_in_place = {
            let hot = self.otel_hot_settings.load();
            OtelInPlaceKey {
                push_interval_ms: hot.push_interval_ms,
                headers: hot.headers.clone(),
                resource_attributes: hot.resource_attributes.clone(),
            }
        };
        let new_otel_in_place = OtelInPlaceKey::from_compiled(new);

        let otel_change = match (
            current_otel_key.endpoint.is_some(),
            new_otel_key.endpoint.is_some(),
        ) {
            (false, false) => OtelChange::None,
            (false, true) => OtelChange::Toggle, // Enable
            (true, false) => OtelChange::Toggle, // Disable
            (true, true) => {
                if current_otel_key != new_otel_key {
                    OtelChange::Reconnect
                } else if current_otel_in_place != new_otel_in_place {
                    OtelChange::InPlace
                } else {
                    OtelChange::None
                }
            }
        };

        // For reconnect, validate we can build the new client (no actual connection yet)
        let new_otel_settings = match otel_change {
            OtelChange::Reconnect | OtelChange::Toggle if new.otel_endpoint.is_some() => {
                Some(OtelPushSettings {
                    endpoint: new.otel_endpoint.clone().unwrap(),
                    push_interval_ms: new.otel_push_interval_ms,
                    resource_attributes: new.otel_resource_attributes.clone(),
                    allow_invalid_certs: new.otel_allow_invalid_certs,
                    headers: new.otel_headers.clone(),
                })
            }
            _ => None,
        };

        Ok(PendingExportChange {
            new_prometheus_listener,
            new_prometheus_path,
            prometheus_change,
            new_otel_settings,
            otel_change,
            compiled: new.clone(),
        })
    }

    /// Commit a prepared change after successful snapshot install.
    ///
    /// Shuts down old sinks and activates new ones.
    pub async fn commit(&self, pending: PendingExportChange, events: Arc<EventHub>) {
        // Handle Prometheus changes
        match pending.prometheus_change {
            PrometheusChange::None => {}
            PrometheusChange::AddressChange => {
                // Take old handle out of mutex (don't hold lock across await)
                let old_handle = self.prometheus.lock().take();

                // Shut down old server first
                if let Some(old_handle) = old_handle {
                    old_handle.shutdown().await;
                    tracing::debug!("old prometheus server stopped");
                }

                // Spawn new server with pre-bound listener
                if let Some(listener) = pending.new_prometheus_listener {
                    let path = pending
                        .new_prometheus_path
                        .unwrap_or_else(|| "/metrics".into());
                    let addr = listener.local_addr().ok();
                    let handle = PrometheusServer::spawn_with_listener(
                        listener,
                        path.clone(),
                        self.hub.clone(),
                        events.clone(),
                    );
                    *self.prometheus.lock() = Some(handle);
                    if let Some(addr) = addr {
                        tracing::info!(%addr, %path, "prometheus metrics rebound to new address");
                    }
                }

                *self.prometheus_key.lock() = PrometheusBindKey::from_compiled(&pending.compiled);
            }
            PrometheusChange::PathChange => {
                // Path-only change: stop old, bind and start new
                let old_handle = self.prometheus.lock().take();

                if let Some(old_handle) = old_handle {
                    old_handle.shutdown().await;
                    tracing::debug!("old prometheus server stopped for path change");
                }

                // Bind and start new server with same address but new path
                if let Some(ref addr_str) = pending.compiled.prometheus_listen {
                    if let Ok(addr) = addr_str.parse::<SocketAddr>() {
                        match TcpListener::bind(addr).await {
                            Ok(listener) => {
                                let path = pending
                                    .new_prometheus_path
                                    .unwrap_or_else(|| "/metrics".into());
                                let handle = PrometheusServer::spawn_with_listener(
                                    listener,
                                    path.clone(),
                                    self.hub.clone(),
                                    events.clone(),
                                );
                                *self.prometheus.lock() = Some(handle);
                                tracing::info!(%addr, %path, "prometheus metrics restarted with new path");
                            }
                            Err(e) => {
                                tracing::error!(%addr, error = %e, "prometheus rebind failed during path change");
                            }
                        }
                    }
                }

                *self.prometheus_key.lock() = PrometheusBindKey::from_compiled(&pending.compiled);
            }
            PrometheusChange::Disable => {
                let old_handle = self.prometheus.lock().take();
                if let Some(old_handle) = old_handle {
                    old_handle.shutdown().await;
                    tracing::debug!("prometheus server stopped (disabled)");
                }
                tracing::info!("prometheus metrics disabled");
                *self.prometheus_key.lock() = PrometheusBindKey::from_compiled(&pending.compiled);
            }
        }

        // Handle OTLP changes
        match pending.otel_change {
            OtelChange::None => {}
            OtelChange::InPlace => {
                // Update hot settings without rebuilding client
                let hot = OtelHotSettings {
                    push_interval_ms: pending.compiled.otel_push_interval_ms,
                    headers: pending.compiled.otel_headers.clone(),
                    resource_attributes: pending.compiled.otel_resource_attributes.clone(),
                };
                self.otel_hot_settings.store(Arc::new(hot));
                tracing::debug!("otel settings updated in place");
            }
            OtelChange::Toggle | OtelChange::Reconnect => {
                // Take old handle out of mutex (don't hold lock across await)
                let old_handle = self.otel.lock().take();

                // Shut down old client
                if let Some(old_handle) = old_handle {
                    old_handle.shutdown().await;
                    tracing::debug!("old otel push stopped");
                }

                // Start new client if settings provided
                if let Some(settings) = pending.new_otel_settings {
                    let endpoint = settings.endpoint.clone();
                    let hot = OtelHotSettings {
                        push_interval_ms: settings.push_interval_ms,
                        headers: settings.headers.clone(),
                        resource_attributes: settings.resource_attributes.clone(),
                    };
                    self.otel_hot_settings.store(Arc::new(hot));

                    let handle = build_otel_push_loop(
                        settings,
                        self.hub.clone(),
                        events,
                        self.otel_hot_settings.clone(),
                    );
                    *self.otel.lock() = Some(handle);
                    tracing::info!(%endpoint, "otel metrics push reconnected");
                } else {
                    tracing::info!("otel metrics push disabled");
                }

                *self.otel_reconnect_key.lock() =
                    OtelReconnectKey::from_compiled(&pending.compiled);
            }
        }
    }

    /// Shut down all active sinks (called during process shutdown).
    pub async fn shutdown(&self) {
        // Take handles out of mutexes (don't hold locks across await)
        let prom_handle = self.prometheus.lock().take();
        let otel_handle = self.otel.lock().take();

        if let Some(handle) = prom_handle {
            handle.shutdown().await;
            tracing::debug!("prometheus metrics stopped");
        }
        if let Some(handle) = otel_handle {
            handle.shutdown().await;
            tracing::debug!("otel metrics push stopped");
        }
    }

    /// Check if plan-only change (no rebind needed for either sink).
    pub fn is_plan_only_change(&self, new: &CompiledMetrics) -> bool {
        let current_prom_key = self.prometheus_key.lock().clone();
        let new_prom_key = PrometheusBindKey::from_compiled(new);
        let current_otel_key = self.otel_reconnect_key.lock().clone();
        let new_otel_key = OtelReconnectKey::from_compiled(new);

        current_prom_key == new_prom_key && current_otel_key == new_otel_key
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use conduit_config::load_yaml;

    fn base_config(prom_port: u16) -> String {
        format!(
            r#"
schema_version: 1
metrics:
  enabled: true
  base: standard
  prometheus:
    listen_address: "127.0.0.1:{prom_port}"
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
        )
    }

    #[test]
    fn prometheus_change_detection_same() {
        let yaml = base_config(19090);
        let cfg = load_yaml(&yaml).unwrap();
        let (compiled, _) = crate::compile_from_config(&cfg);

        let key1 = PrometheusBindKey::from_compiled(&compiled);
        let key2 = PrometheusBindKey::from_compiled(&compiled);
        assert_eq!(key1, key2);
    }

    #[test]
    fn prometheus_change_detection_port_change() {
        let yaml1 = base_config(19090);
        let cfg1 = load_yaml(&yaml1).unwrap();
        let (compiled1, _) = crate::compile_from_config(&cfg1);

        let yaml2 = base_config(19091);
        let cfg2 = load_yaml(&yaml2).unwrap();
        let (compiled2, _) = crate::compile_from_config(&cfg2);

        let key1 = PrometheusBindKey::from_compiled(&compiled1);
        let key2 = PrometheusBindKey::from_compiled(&compiled2);
        assert_ne!(key1, key2);
    }

    #[test]
    fn prometheus_change_detection_path_change() {
        let yaml1 = r#"
schema_version: 1
metrics:
  enabled: true
  base: standard
  prometheus:
    listen_address: "127.0.0.1:19090"
    path: /metrics
pools:
  - name: default
    backends:
      - address: "127.0.0.1:5300"
listeners:
  listeners:
    - address: "127.0.0.1:15353"
      protocol: udp
"#;
        let yaml2 = r#"
schema_version: 1
metrics:
  enabled: true
  base: standard
  prometheus:
    listen_address: "127.0.0.1:19090"
    path: /custom-metrics
pools:
  - name: default
    backends:
      - address: "127.0.0.1:5300"
listeners:
  listeners:
    - address: "127.0.0.1:15353"
      protocol: udp
"#;
        let cfg1 = load_yaml(yaml1).unwrap();
        let (compiled1, _) = crate::compile_from_config(&cfg1);
        let cfg2 = load_yaml(yaml2).unwrap();
        let (compiled2, _) = crate::compile_from_config(&cfg2);

        let key1 = PrometheusBindKey::from_compiled(&compiled1);
        let key2 = PrometheusBindKey::from_compiled(&compiled2);
        assert_ne!(key1, key2, "path change should trigger rebind");
    }

    #[test]
    fn otel_change_detection_endpoint_change() {
        let yaml1 = r#"
schema_version: 1
metrics:
  enabled: true
  base: standard
  otel:
    endpoint: "http://127.0.0.1:4318/v1/metrics"
pools:
  - name: default
    backends:
      - address: "127.0.0.1:5300"
listeners:
  listeners:
    - address: "127.0.0.1:15353"
      protocol: udp
"#;
        let yaml2 = r#"
schema_version: 1
metrics:
  enabled: true
  base: standard
  otel:
    endpoint: "http://127.0.0.1:4319/v1/metrics"
pools:
  - name: default
    backends:
      - address: "127.0.0.1:5300"
listeners:
  listeners:
    - address: "127.0.0.1:15353"
      protocol: udp
"#;
        let cfg1 = load_yaml(yaml1).unwrap();
        let (compiled1, _) = crate::compile_from_config(&cfg1);
        let cfg2 = load_yaml(yaml2).unwrap();
        let (compiled2, _) = crate::compile_from_config(&cfg2);

        let key1 = OtelReconnectKey::from_compiled(&compiled1);
        let key2 = OtelReconnectKey::from_compiled(&compiled2);
        assert_ne!(key1, key2, "endpoint change should trigger reconnect");
    }

    #[test]
    fn otel_in_place_update_interval_only() {
        let yaml1 = r#"
schema_version: 1
metrics:
  enabled: true
  base: standard
  otel:
    endpoint: "http://127.0.0.1:4318/v1/metrics"
    push_interval_ms: 15000
pools:
  - name: default
    backends:
      - address: "127.0.0.1:5300"
listeners:
  listeners:
    - address: "127.0.0.1:15353"
      protocol: udp
"#;
        let yaml2 = r#"
schema_version: 1
metrics:
  enabled: true
  base: standard
  otel:
    endpoint: "http://127.0.0.1:4318/v1/metrics"
    push_interval_ms: 30000
pools:
  - name: default
    backends:
      - address: "127.0.0.1:5300"
listeners:
  listeners:
    - address: "127.0.0.1:15353"
      protocol: udp
"#;
        let cfg1 = load_yaml(yaml1).unwrap();
        let (compiled1, _) = crate::compile_from_config(&cfg1);
        let cfg2 = load_yaml(yaml2).unwrap();
        let (compiled2, _) = crate::compile_from_config(&cfg2);

        let reconnect1 = OtelReconnectKey::from_compiled(&compiled1);
        let reconnect2 = OtelReconnectKey::from_compiled(&compiled2);
        assert_eq!(
            reconnect1, reconnect2,
            "interval change should NOT trigger reconnect"
        );

        let inplace1 = OtelInPlaceKey::from_compiled(&compiled1);
        let inplace2 = OtelInPlaceKey::from_compiled(&compiled2);
        assert_ne!(
            inplace1, inplace2,
            "interval change should trigger in-place update"
        );
    }
}
