//! Per-worker upstream UDP egress (IPv4/IPv6 sources + round-robin).

use conduit_config::forward::CompiledForward;
use std::collections::HashMap;
use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr, UdpSocket};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

/// Default bind when no `sources_v4` are configured.
pub const DEFAULT_BIND_V4: Ipv4Addr = Ipv4Addr::UNSPECIFIED;
/// Default bind when no `sources_v6` are configured.
pub const DEFAULT_BIND_V6: Ipv6Addr = Ipv6Addr::UNSPECIFIED;

/// Per-attempt source selection inputs for `udp_socket_for`.
pub struct EgressSourceSelection<'a> {
    pub pool_sources_v4: &'a [Ipv4Addr],
    pub pool_sources_v6: &'a [Ipv6Addr],
    pub backend: SocketAddr,
    pub override_v4: Option<Ipv4Addr>,
    pub allowed_v4: &'a [Ipv4Addr],
    pub override_v6: Option<Ipv6Addr>,
    pub allowed_v6: &'a [Ipv6Addr],
}

pub struct WorkerForwardEgress {
    sockets_v4: HashMap<Ipv4Addr, UdpSocket>,
    sockets_v6: HashMap<Ipv6Addr, UdpSocket>,
    default_socket_v4: UdpSocket,
    default_socket_v6: UdpSocket,
    forward_sources_v4: Vec<Ipv4Addr>,
    forward_sources_v6: Vec<Ipv6Addr>,
    rr_index_v4: AtomicUsize,
    rr_index_v6: AtomicUsize,
}

impl WorkerForwardEgress {
    /// `bind_addresses_v4` / `bind_addresses_v6` must include every address that may be selected.
    pub fn new(
        compiled: &CompiledForward,
        bind_addresses_v4: &[Ipv4Addr],
        bind_addresses_v6: &[Ipv6Addr],
        timeout_ms: u32,
    ) -> std::io::Result<Self> {
        let timeout = Duration::from_millis(timeout_ms as u64);
        let mut sockets_v4 = HashMap::new();
        for addr in bind_addresses_v4 {
            sockets_v4.insert(*addr, bind_udp_v4(*addr, timeout)?);
        }
        let mut sockets_v6 = HashMap::new();
        for addr in bind_addresses_v6 {
            sockets_v6.insert(*addr, bind_udp_v6(*addr, timeout)?);
        }
        Ok(Self {
            default_socket_v4: bind_udp_v4(DEFAULT_BIND_V4, timeout)?,
            default_socket_v6: bind_udp_v6(DEFAULT_BIND_V6, timeout)?,
            sockets_v4,
            sockets_v6,
            forward_sources_v4: compiled.sources_v4.clone(),
            forward_sources_v6: compiled.sources_v6.clone(),
            rr_index_v4: AtomicUsize::new(0),
            rr_index_v6: AtomicUsize::new(0),
        })
    }

    /// Select IPv4 source for this attempt (pool → forward RR). Honors script override when allowed.
    pub fn select_source_v4(
        &self,
        pool_sources: &[Ipv4Addr],
        override_addr: Option<Ipv4Addr>,
        allowed: &[Ipv4Addr],
    ) -> Ipv4Addr {
        if let Some(addr) = override_addr {
            if allowed.contains(&addr) {
                return addr;
            }
        }
        self.round_robin_v4(pool_sources)
    }

    fn round_robin_v4(&self, pool_sources: &[Ipv4Addr]) -> Ipv4Addr {
        let list = if !pool_sources.is_empty() {
            pool_sources
        } else if !self.forward_sources_v4.is_empty() {
            &self.forward_sources_v4
        } else {
            return DEFAULT_BIND_V4;
        };
        let idx = self.rr_index_v4.fetch_add(1, Ordering::Relaxed);
        list[idx % list.len()]
    }

    /// Select IPv6 source for this attempt (pool → forward RR). Honors script override when allowed.
    pub fn select_source_v6(
        &self,
        pool_sources: &[Ipv6Addr],
        override_addr: Option<Ipv6Addr>,
        allowed: &[Ipv6Addr],
    ) -> Ipv6Addr {
        if let Some(addr) = override_addr {
            if allowed.contains(&addr) {
                return addr;
            }
        }
        self.round_robin_v6(pool_sources)
    }

    fn round_robin_v6(&self, pool_sources: &[Ipv6Addr]) -> Ipv6Addr {
        let list = if !pool_sources.is_empty() {
            pool_sources
        } else if !self.forward_sources_v6.is_empty() {
            &self.forward_sources_v6
        } else {
            return DEFAULT_BIND_V6;
        };
        let idx = self.rr_index_v6.fetch_add(1, Ordering::Relaxed);
        list[idx % list.len()]
    }

    /// UDP socket for upstream send/receive matching backend address family.
    pub fn udp_socket_for(&self, sel: &EgressSourceSelection<'_>) -> &UdpSocket {
        if sel.backend.is_ipv4() {
            let addr = self.select_source_v4(sel.pool_sources_v4, sel.override_v4, sel.allowed_v4);
            return self
                .sockets_v4
                .get(&addr)
                .unwrap_or(&self.default_socket_v4);
        }
        let addr = self.select_source_v6(sel.pool_sources_v6, sel.override_v6, sel.allowed_v6);
        self.sockets_v6
            .get(&addr)
            .unwrap_or(&self.default_socket_v6)
    }
}

fn bind_udp_v4(addr: Ipv4Addr, timeout: Duration) -> std::io::Result<UdpSocket> {
    let socket = UdpSocket::bind(SocketAddr::from((addr, 0)))?;
    socket.set_read_timeout(Some(timeout))?;
    Ok(socket)
}

fn bind_udp_v6(addr: Ipv6Addr, timeout: Duration) -> std::io::Result<UdpSocket> {
    let socket = UdpSocket::bind(SocketAddr::from((addr, 0)))?;
    socket.set_read_timeout(Some(timeout))?;
    Ok(socket)
}

#[cfg(test)]
mod tests {
    use super::*;
    use conduit_config::forward::{CompiledForward, UpstreamTransport};
    use std::net::UdpSocket;
    use std::sync::mpsc;
    use std::thread;
    use std::time::Duration;

    fn compiled_v4_only() -> CompiledForward {
        CompiledForward {
            sources_v4: vec![Ipv4Addr::UNSPECIFIED, Ipv4Addr::LOCALHOST],
            sources_v6: vec![],
            source_selection: "round_robin".into(),
            upstream_transport: UpstreamTransport::UdpOnly,
            client_tcp_uses_upstream_tcp: false,
            timeout_ms: 1000,
            outstanding_per_backend: 10,
        }
    }

    #[test]
    fn round_robin_alternates_local_ports() {
        let compiled = compiled_v4_only();
        let (tx, rx) = mpsc::channel();
        let server = thread::spawn(move || {
            let sock = UdpSocket::bind("127.0.0.1:0").unwrap();
            let port = sock.local_addr().unwrap().port();
            tx.send(port).unwrap();
            let mut buf = [0u8; 512];
            let mut ports = Vec::new();
            sock.set_read_timeout(Some(Duration::from_millis(500)))
                .unwrap();
            while ports.len() < 2 {
                if let Ok((_, peer)) = sock.recv_from(&mut buf) {
                    ports.push(peer.port());
                }
            }
            ports
        });
        let backend_port = rx.recv().unwrap();
        let backend: SocketAddr = format!("127.0.0.1:{backend_port}").parse().unwrap();

        let bind = vec![Ipv4Addr::UNSPECIFIED, Ipv4Addr::LOCALHOST];
        let egress = WorkerForwardEgress::new(&compiled, &bind, &[], 1000).unwrap();
        let msg = [0u8; 12];
        for _ in 0..2 {
            let sel = EgressSourceSelection {
                pool_sources_v4: &[],
                pool_sources_v6: &[],
                backend,
                override_v4: None,
                allowed_v4: &bind,
                override_v6: None,
                allowed_v6: &[],
            };
            let sock = egress.udp_socket_for(&sel);
            sock.send_to(&msg, backend).unwrap();
        }
        let ports = server.join().unwrap();
        assert_ne!(
            ports[0], ports[1],
            "expected different ephemeral source ports"
        );
    }

    #[test]
    fn pool_override_binds_configured_source_ip() {
        let compiled = CompiledForward {
            sources_v4: vec![],
            sources_v6: vec![],
            source_selection: "round_robin".into(),
            upstream_transport: UpstreamTransport::UdpOnly,
            client_tcp_uses_upstream_tcp: false,
            timeout_ms: 1000,
            outstanding_per_backend: 10,
        };
        let pool_source = Ipv4Addr::LOCALHOST;
        let bind = vec![pool_source];
        let (tx, rx) = mpsc::channel();
        let server = thread::spawn(move || {
            let sock = UdpSocket::bind("127.0.0.1:0").unwrap();
            let port = sock.local_addr().unwrap().port();
            tx.send(port).unwrap();
            let mut buf = [0u8; 512];
            let (_, peer) = sock.recv_from(&mut buf).unwrap();
            peer
        });
        let backend_port = rx.recv().unwrap();
        let backend: SocketAddr = format!("127.0.0.1:{backend_port}").parse().unwrap();

        let egress = WorkerForwardEgress::new(&compiled, &bind, &[], 1000).unwrap();
        let sel = EgressSourceSelection {
            pool_sources_v4: &[pool_source],
            pool_sources_v6: &[],
            backend,
            override_v4: None,
            allowed_v4: &bind,
            override_v6: None,
            allowed_v6: &[],
        };
        let sock = egress.udp_socket_for(&sel);
        sock.send_to(&[0u8; 12], backend).unwrap();
        let peer = server.join().unwrap();
        assert_eq!(peer.ip(), pool_source);
    }

    #[test]
    fn source_override_when_allowed() {
        let compiled = compiled_v4_only();
        let bind = vec![Ipv4Addr::LOCALHOST];
        let egress = WorkerForwardEgress::new(&compiled, &bind, &[], 1000).unwrap();
        let addr = egress.select_source_v4(&[], Some(Ipv4Addr::LOCALHOST), &bind);
        assert_eq!(addr, Ipv4Addr::LOCALHOST);
    }

    #[test]
    fn source_override_v6_when_allowed() {
        let compiled = CompiledForward {
            sources_v4: vec![],
            sources_v6: vec![Ipv6Addr::LOCALHOST],
            source_selection: "round_robin".into(),
            upstream_transport: UpstreamTransport::UdpOnly,
            client_tcp_uses_upstream_tcp: false,
            timeout_ms: 1000,
            outstanding_per_backend: 10,
        };
        let bind = vec![Ipv6Addr::LOCALHOST];
        let egress = WorkerForwardEgress::new(&compiled, &[], &bind, 1000).unwrap();
        let addr = egress.select_source_v6(&[], Some(Ipv6Addr::LOCALHOST), &bind);
        assert_eq!(addr, Ipv6Addr::LOCALHOST);
    }
}
