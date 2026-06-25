//! Per-listener ingress settings with block-level inheritance.

use conduit_proto::config::{Listener, ListenersConfig};

/// Resolved ingress settings for one listener entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedListenerIngress {
    pub threads: u32,
    pub reuse_port: bool,
    pub rcvbuf: u32,
    /// Stable identity when set; otherwise derived from address/protocol.
    pub name: String,
}

/// Resolve per-listener overrides against block-level defaults.
pub fn resolve_listener_ingress(
    block: &ListenersConfig,
    listener: &Listener,
) -> ResolvedListenerIngress {
    let threads = listener.threads.unwrap_or(block.threads).max(1);
    let reuse_port = listener.reuse_port.unwrap_or(block.reuse_port);
    let rcvbuf = listener.rcvbuf.unwrap_or(block.rcvbuf);
    let name = listener
        .name
        .as_ref()
        .filter(|n| !n.is_empty())
        .cloned()
        .unwrap_or_else(|| default_listener_name(listener));
    ResolvedListenerIngress {
        threads,
        reuse_port,
        rcvbuf,
        name,
    }
}

fn default_listener_name(listener: &Listener) -> String {
    format!("{}:{}", listener.protocol.to_lowercase(), listener.address)
}

pub fn validate_listeners(cfg: &ListenersConfig) -> Vec<String> {
    let mut errors = Vec::new();

    if cfg.threads == 0 {
        errors.push("listeners.threads must be >= 1".into());
    }

    let mut names = std::collections::HashSet::new();
    for ln in &cfg.listeners {
        if ln.address.is_empty() {
            errors.push("listener address must not be empty".into());
        }
        if let Some(threads) = ln.threads {
            if threads == 0 {
                errors.push(format!("listener '{}' threads must be >= 1", ln.address));
            }
        }
        if let Some(name) = ln.name.as_ref().filter(|n| !n.is_empty()) {
            if !names.insert(name.clone()) {
                errors.push(format!("duplicate listener name '{name}'"));
            }
        }
    }

    errors
}

#[cfg(test)]
mod tests {
    use super::*;

    fn block() -> ListenersConfig {
        ListenersConfig {
            threads: 2,
            reuse_port: false,
            rcvbuf: 0,
            sndbuf: 0,
            listeners: vec![],
        }
    }

    #[test]
    fn inherit_block_defaults() {
        let block = block();
        let ln = Listener {
            address: "127.0.0.1:53".into(),
            protocol: "udp".into(),
            threads: None,
            reuse_port: None,
            name: None,
            rcvbuf: None,
        };
        let resolved = resolve_listener_ingress(&block, &ln);
        assert_eq!(resolved.threads, 2);
        assert!(!resolved.reuse_port);
        assert_eq!(resolved.name, "udp:127.0.0.1:53");
    }

    #[test]
    fn per_listener_overrides() {
        let block = block();
        let ln = Listener {
            address: "127.0.0.1:53".into(),
            protocol: "udp".into(),
            threads: Some(8),
            reuse_port: Some(true),
            name: Some("public-udp".into()),
            rcvbuf: Some(1_048_576),
        };
        let resolved = resolve_listener_ingress(&block, &ln);
        assert_eq!(resolved.threads, 8);
        assert!(resolved.reuse_port);
        assert_eq!(resolved.rcvbuf, 1_048_576);
        assert_eq!(resolved.name, "public-udp");
    }
}
