//! Operator-facing startup summary for the active snapshot.

use conduit_core::snapshot::RuntimeSnapshot;
use conduit_events::EventHub;

/// Log a concise summary of dataplane-related config (generation, listeners, egress, event sinks).
pub fn log_startup_summary(snap: &RuntimeSnapshot, events_hub: &EventHub) {
    let cfg = &snap.config;
    let listener_count = cfg
        .listeners
        .as_ref()
        .map(|l| l.listeners.len())
        .unwrap_or(0);
    let pool_count = cfg.pools.len();
    let rule_count = cfg.rules.as_ref().map(|r| r.rules.len()).unwrap_or(0);
    let egress_v4 = snap.egress_bind_addresses_v4();
    let egress_v6 = snap.egress_bind_addresses_v6();

    tracing::info!(
        generation = snap.generation,
        listeners = listener_count,
        pools = pool_count,
        rules = rule_count,
        forward_timeout_ms = snap.forward.timeout_ms,
        egress_sources_v4 = ?egress_v4,
        egress_sources_v6 = ?egress_v6,
        event_sinks = events_hub.consumer_count(),
        events_enabled = events_hub.enabled(),
        "dataplane startup summary"
    );

    if let Some(listeners) = cfg.listeners.as_ref() {
        for ln in &listeners.listeners {
            tracing::info!(
                address = %ln.address,
                protocol = %ln.protocol,
                worker_threads = listeners.threads.max(1),
                "configured listener"
            );
        }
    }
}

/// Log after a listener socket has bound successfully.
pub fn log_listener_bound(addr: std::net::SocketAddr, protocol: &str) {
    let proto = normalize_protocol(protocol);
    tracing::info!("Starting listening on {addr} {proto}");
}

fn normalize_protocol(protocol: &str) -> &str {
    match protocol.to_ascii_lowercase().as_str() {
        "tcp" => "tcp",
        _ => "udp",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_protocol_values() {
        assert_eq!(normalize_protocol("UDP"), "udp");
        assert_eq!(normalize_protocol("tcp"), "tcp");
        assert_eq!(normalize_protocol(""), "udp");
    }
}
