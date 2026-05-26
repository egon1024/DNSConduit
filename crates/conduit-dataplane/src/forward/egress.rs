//! Per-worker IPv4 upstream UDP egress (sources + round-robin).

use conduit_config::forward::CompiledForward;
use std::collections::HashMap;
use std::net::{Ipv4Addr, SocketAddr, UdpSocket};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

/// Default bind when no `sources_v4` are configured.
pub const DEFAULT_BIND_V4: Ipv4Addr = Ipv4Addr::UNSPECIFIED;

pub struct WorkerForwardEgress {
    sockets: HashMap<Ipv4Addr, UdpSocket>,
    default_socket: UdpSocket,
    forward_sources: Vec<Ipv4Addr>,
    rr_index: AtomicUsize,
}

impl WorkerForwardEgress {
    /// `bind_addresses_v4` must include every address that may be selected by `socket_for_pool`
    /// (union of `forward.sources_v4` and all pool `sources_v4` overrides).
    pub fn new(
        compiled: &CompiledForward,
        bind_addresses_v4: &[Ipv4Addr],
        timeout_ms: u32,
    ) -> std::io::Result<Self> {
        let timeout = Duration::from_millis(timeout_ms as u64);
        let mut sockets = HashMap::new();
        for addr in bind_addresses_v4 {
            sockets.insert(*addr, bind_source(*addr, timeout)?);
        }
        let default_socket = bind_source(DEFAULT_BIND_V4, timeout)?;
        Ok(Self {
            sockets,
            default_socket,
            forward_sources: compiled.sources_v4.clone(),
            rr_index: AtomicUsize::new(0),
        })
    }

    /// Select socket for this attempt using pool → forward source list precedence.
    pub fn socket_for_pool(&self, pool_sources: &[Ipv4Addr]) -> &UdpSocket {
        let list = if !pool_sources.is_empty() {
            pool_sources
        } else if !self.forward_sources.is_empty() {
            &self.forward_sources
        } else {
            return &self.default_socket;
        };
        let idx = self.rr_index.fetch_add(1, Ordering::Relaxed);
        let addr = list[idx % list.len()];
        self.sockets.get(&addr).unwrap_or(&self.default_socket)
    }
}

fn bind_source(addr: Ipv4Addr, timeout: Duration) -> std::io::Result<UdpSocket> {
    let bind_addr = SocketAddr::from((addr, 0));
    let socket = UdpSocket::bind(bind_addr)?;
    socket.set_read_timeout(Some(timeout))?;
    Ok(socket)
}

#[cfg(test)]
mod tests {
    use super::*;
    use conduit_config::forward::{CompiledForward, RecursionDesired};
    use std::net::UdpSocket;
    use std::sync::mpsc;
    use std::thread;
    use std::time::Duration;

    #[test]
    fn round_robin_alternates_local_ports() {
        let compiled = CompiledForward {
            sources_v4: vec![Ipv4Addr::UNSPECIFIED, Ipv4Addr::LOCALHOST],
            source_selection: "round_robin".into(),
            recursion_desired: RecursionDesired::Preserve,
            timeout_ms: 1000,
            outstanding_per_backend: 10,
        };
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
        let egress = WorkerForwardEgress::new(&compiled, &bind, 1000).unwrap();
        let msg = [0u8; 12];
        for _ in 0..2 {
            let sock = egress.socket_for_pool(&[]);
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
            source_selection: "round_robin".into(),
            recursion_desired: RecursionDesired::Preserve,
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

        let egress = WorkerForwardEgress::new(&compiled, &bind, 1000).unwrap();
        let sock = egress.socket_for_pool(&[pool_source]);
        sock.send_to(&[0u8; 12], backend).unwrap();
        let peer = server.join().unwrap();
        assert_eq!(peer.ip(), pool_source);
    }
}
