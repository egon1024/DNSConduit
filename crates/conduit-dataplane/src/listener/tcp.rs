//! TCP DNS listener (RFC 1035 length-prefixed).

use crate::listener::DataplaneShutdown;
use conduit_core::orchestrator::{Orchestrator, RunOutcome};
use conduit_core::snapshot::SnapshotStore;
use conduit_core::transaction::{ClientProtocol, Transaction};
use conduit_events::EventHub;
use conduit_proto::config::Listener;
use socket2::{Domain, Protocol, Socket, Type};
use std::io::{Read, Write};
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

pub fn bind_socket(listener: &Listener) -> std::io::Result<(Socket, SocketAddr)> {
    let addr: SocketAddr = listener
        .address
        .parse()
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidInput, e))?;

    let domain = if addr.is_ipv4() {
        Domain::IPV4
    } else {
        Domain::IPV6
    };
    let socket = Socket::new(domain, Type::STREAM, Some(Protocol::TCP))?;
    socket.set_reuse_address(true)?;
    socket.bind(&addr.into())?;
    socket.listen(128)?;
    let bound = socket.local_addr()?.as_socket().unwrap();
    Ok((socket, bound))
}

pub fn run_worker(
    tcp: std::net::TcpListener,
    listener: Listener,
    store: Arc<SnapshotStore>,
    orchestrator: Arc<Orchestrator>,
    observation: Arc<EventHub>,
    shutdown: DataplaneShutdown,
) -> std::io::Result<()> {
    tcp.set_nonblocking(true)?;
    let mut next_id = 1u64;
    loop {
        if shutdown.is_shutdown() {
            break;
        }
        match tcp.accept() {
            Ok((mut stream, _)) => {
                let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
                let peer = match stream.peer_addr() {
                    Ok(p) => p,
                    Err(_) => continue,
                };
                let mut len_buf = [0u8; 2];
                if stream.read_exact(&mut len_buf).is_err() {
                    continue;
                }
                let len = u16::from_be_bytes(len_buf) as usize;
                if len == 0 || len > 65535 {
                    continue;
                }
                let mut buf = vec![0u8; len];
                if stream.read_exact(&mut buf).is_err() {
                    continue;
                }
                let snap = store.load();
                let mut txn = Transaction::new(next_id, peer, ClientProtocol::Tcp)
                    .with_listener_label(listener.address.clone())
                    .with_query_wire(buf);
                next_id = next_id.wrapping_add(1);
                if let RunOutcome::Response(wire) = orchestrator.run(
                    &mut txn,
                    &snap,
                    &conduit_core::SystemClock,
                    Some(observation.as_ref()),
                ) {
                    let len = (wire.len() as u16).to_be_bytes();
                    let _ = stream.write_all(&len);
                    let _ = stream.write_all(&wire);
                }
            }
            Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                if shutdown.is_shutdown() {
                    break;
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(_) if shutdown.is_shutdown() => break,
            Err(e) => return Err(e),
        }
    }
    Ok(())
}
