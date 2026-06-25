//! UDP DNS listener worker.

use crate::listener::DataplaneShutdown;
use crate::query_slot::run_in_slot;
use conduit_core::orchestrator::Orchestrator;
use conduit_core::snapshot::SnapshotStore;
use conduit_core::transaction::{ClientProtocol, Transaction};
use conduit_core::txn_store::SharedTxnStore;
use conduit_events::EventHub;
use conduit_proto::config::Listener;
use socket2::{Domain, Protocol, Socket, Type};
use std::net::SocketAddr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

pub fn bind_socket(
    listener: &Listener,
    reuse_port: bool,
    rcvbuf: u32,
) -> std::io::Result<(Socket, SocketAddr)> {
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
    let bound = socket.local_addr()?.as_socket().unwrap();
    Ok((socket, bound))
}

#[allow(clippy::too_many_arguments)]
pub fn run_worker(
    udp: std::net::UdpSocket,
    listener: Listener,
    store: Arc<SnapshotStore>,
    txn_store: SharedTxnStore,
    orchestrator: Arc<Orchestrator>,
    observation: Arc<EventHub>,
    shutdown: DataplaneShutdown,
    global_query_counter: Arc<AtomicU64>,
) -> std::io::Result<()> {
    udp.set_read_timeout(Some(Duration::from_secs(1)))?;

    let mut buf = [0u8; 4096];
    let mut next_id = 1u64;
    loop {
        if shutdown.is_shutdown() {
            break;
        }
        match udp.recv_from(&mut buf) {
            Ok((len, peer)) => {
                let snap = store.load();
                let global_query_index = global_query_counter.fetch_add(1, Ordering::Relaxed) + 1;
                let query_bytes = buf[..len].to_vec();
                let wire = match run_in_slot(
                    &txn_store,
                    orchestrator.as_ref(),
                    &snap,
                    observation.as_ref(),
                    |slot| {
                        slot.txn = Transaction::new(next_id, peer, ClientProtocol::Udp)
                            .with_global_query_index(global_query_index)
                            .with_listener_label(listener.address.clone())
                            .with_query_wire(query_bytes.clone());
                        let _ = slot.query.set_from_slice(&query_bytes);
                    },
                ) {
                    Ok(w) => w,
                    Err(_) => {
                        tracing::debug!(txn_id = next_id, "slot pool exhausted; dropping query");
                        None
                    }
                };
                next_id = next_id.wrapping_add(1);
                if let Some(wire) = wire {
                    let _ = udp.send_to(&wire, peer);
                }
            }
            Err(ref e)
                if e.kind() == std::io::ErrorKind::WouldBlock
                    || e.kind() == std::io::ErrorKind::TimedOut =>
            {
                if shutdown.is_shutdown() {
                    break;
                }
                continue;
            }
            Err(_) if shutdown.is_shutdown() => break,
            Err(e) => return Err(e),
        }
    }
    Ok(())
}
