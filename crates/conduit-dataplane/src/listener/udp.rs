//! UDP DNS listener worker.

use crate::listener::startup_log;
use conduit_core::orchestrator::{Orchestrator, RunOutcome};
use conduit_core::snapshot::SnapshotStore;
use conduit_core::transaction::{ClientProtocol, Transaction};
use conduit_events::EventHub;
use conduit_proto::config::Listener;
use socket2::{Domain, Protocol, Socket, Type};
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

pub fn run_worker(
    listener: Listener,
    store: Arc<SnapshotStore>,
    orchestrator: Arc<Orchestrator>,
    observation: Arc<EventHub>,
    reuse_port: bool,
    rcvbuf: u32,
) -> std::io::Result<()> {
    let addr: SocketAddr = listener
        .address
        .parse()
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidInput, e))?;

    let domain = if addr.is_ipv4() {
        Domain::IPV4
    } else {
        Domain::IPV6
    };
    let socket = Socket::new(domain, Type::DGRAM, Some(Protocol::UDP))?;
    if reuse_port {
        #[cfg(unix)]
        socket.set_reuse_port(true)?;
    }
    if rcvbuf > 0 {
        socket.set_recv_buffer_size(rcvbuf as usize)?;
    }
    socket.set_nonblocking(false)?;
    socket.bind(&addr.into())?;
    startup_log::log_listener_bound(addr, &listener.protocol);
    let udp: std::net::UdpSocket = socket.into();
    udp.set_read_timeout(Some(Duration::from_secs(1)))?;

    let mut buf = [0u8; 4096];
    let mut next_id = 1u64;
    loop {
        match udp.recv_from(&mut buf) {
            Ok((len, peer)) => {
                let snap = store.load();
                let mut txn = Transaction::new(next_id, peer, ClientProtocol::Udp)
                    .with_listener_label(listener.address.clone())
                    .with_query_wire(buf[..len].to_vec());
                next_id = next_id.wrapping_add(1);
                if let RunOutcome::Response(wire) = orchestrator.run(
                    &mut txn,
                    &snap,
                    &conduit_core::SystemClock,
                    Some(observation.as_ref()),
                ) {
                    let _ = udp.send_to(&wire, peer);
                }
            }
            Err(ref e)
                if e.kind() == std::io::ErrorKind::WouldBlock
                    || e.kind() == std::io::ErrorKind::TimedOut =>
            {
                continue;
            }
            Err(e) => return Err(e),
        }
    }
}
