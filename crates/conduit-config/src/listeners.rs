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

        // Multiple UDP ingress workers bind the same address; without SO_REUSEPORT the
        // second bind fails at startup with EADDRINUSE (often after the first worker
        // already claimed the port, which looks like "nothing listening" in ss).
        let resolved = resolve_listener_ingress(cfg, ln);
        let is_udp = ln.protocol.eq_ignore_ascii_case("udp");
        if is_udp && resolved.threads > 1 && !resolved.reuse_port {
            errors.push(format!(
                "listener '{}' (UDP): threads is {} but reuse_port is false; \
                 set listeners.reuse_port: true (or this listener's reuse_port: true) \
                 so multiple ingress workers can bind the same address, or set threads: 1",
                resolved.name, resolved.threads
            ));
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
            acls: None,
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
            acls: None,
        };
        let resolved = resolve_listener_ingress(&block, &ln);
        assert_eq!(resolved.threads, 8);
        assert!(resolved.reuse_port);
        assert_eq!(resolved.rcvbuf, 1_048_576);
        assert_eq!(resolved.name, "public-udp");
    }

    #[test]
    fn udp_threads_gt_one_requires_reuse_port() {
        let mut cfg = block();
        cfg.threads = 4;
        cfg.reuse_port = false;
        cfg.listeners = vec![Listener {
            address: "127.0.2.1:15353".into(),
            protocol: "udp".into(),
            threads: None,
            reuse_port: None,
            name: Some("lab-udp".into()),
            rcvbuf: None,
            acls: None,
        }];
        let errors = validate_listeners(&cfg);
        assert!(
            errors.iter().any(|e| {
                e.contains("reuse_port")
                    && e.contains("threads")
                    && (e.contains("lab-udp") || e.contains("127.0.2.1:15353"))
            }),
            "expected reuse_port/threads validation error, got {errors:?}"
        );
    }

    #[test]
    fn udp_threads_gt_one_ok_with_reuse_port() {
        let mut cfg = block();
        cfg.threads = 4;
        cfg.reuse_port = true;
        cfg.listeners = vec![Listener {
            address: "127.0.2.1:15353".into(),
            protocol: "udp".into(),
            threads: None,
            reuse_port: None,
            name: Some("lab-udp".into()),
            rcvbuf: None,
            acls: None,
        }];
        assert!(
            validate_listeners(&cfg).is_empty(),
            "{:?}",
            validate_listeners(&cfg)
        );
    }

    #[test]
    fn tcp_threads_gt_one_without_reuse_port_ok() {
        let mut cfg = block();
        cfg.threads = 4;
        cfg.reuse_port = false;
        cfg.listeners = vec![Listener {
            address: "127.0.2.1:15353".into(),
            protocol: "tcp".into(),
            threads: None,
            reuse_port: None,
            name: Some("lab-tcp".into()),
            rcvbuf: None,
            acls: None,
        }];
        assert!(
            validate_listeners(&cfg).is_empty(),
            "{:?}",
            validate_listeners(&cfg)
        );
    }

    #[test]
    fn per_listener_reuse_port_override_satisfies_threads() {
        let mut cfg = block();
        cfg.threads = 4;
        cfg.reuse_port = false;
        cfg.listeners = vec![Listener {
            address: "127.0.2.1:15353".into(),
            protocol: "udp".into(),
            threads: None,
            reuse_port: Some(true),
            name: Some("lab-udp".into()),
            rcvbuf: None,
            acls: None,
        }];
        assert!(
            validate_listeners(&cfg).is_empty(),
            "{:?}",
            validate_listeners(&cfg)
        );
    }
}
