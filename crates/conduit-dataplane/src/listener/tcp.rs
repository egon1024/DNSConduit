//! TCP DNS listener (RFC 1035 length-prefixed).

use crate::acl_gate::{AclGate, AclGateOutcome};
use crate::listener::DataplaneShutdown;
use crate::query_slot::run_in_slot;
use conduit_core::orchestrator::Orchestrator;
use conduit_core::routing::listener_metric_label;
use conduit_core::snapshot::SnapshotStore;
use conduit_core::stages::send::build_error_response;
use conduit_core::structural_parse::{apply_parsed_query, structural_parse};
use conduit_core::transaction::{ClientProtocol, Transaction};
use conduit_core::txn_store::SharedTxnStore;
use conduit_events::EventHub;
use conduit_metrics::MetricsHub;
use conduit_proto::config::Listener;
use socket2::{Domain, Protocol, Socket, Type};
use std::io::{Read, Write};
use std::net::SocketAddr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

/// DNS REFUSED response code.
const RCODE_REFUSED: u16 = 5;

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

#[allow(clippy::too_many_arguments)]
pub fn run_worker(
    tcp: std::net::TcpListener,
    listener: Listener,
    store: Arc<SnapshotStore>,
    txn_store: SharedTxnStore,
    orchestrator: Arc<Orchestrator>,
    observation: Arc<EventHub>,
    metrics: Arc<MetricsHub>,
    shutdown: DataplaneShutdown,
    global_query_counter: Arc<AtomicU64>,
) -> std::io::Result<()> {
    tcp.set_nonblocking(true)?;
    let listener_label = listener_metric_label(&listener);
    let mut acl_gate = AclGate::new(listener.acls.clone(), listener_label.clone());
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
                let snap = store.load();
                // Tier 0: explicit drop closes the TCP session before reading a query.
                if matches!(
                    acl_gate.decide_preadmission(&snap, peer.ip(), &metrics),
                    AclGateOutcome::Drop
                ) {
                    next_id = next_id.wrapping_add(1);
                    continue;
                }
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
                let global_query_index = global_query_counter.fetch_add(1, Ordering::Relaxed) + 1;

                let parsed = match structural_parse(&buf) {
                    Ok(parsed) => parsed,
                    Err(reason) => {
                        metrics.builtin.record_parse_rejected(reason.as_str());
                        next_id = next_id.wrapping_add(1);
                        continue;
                    }
                };

                let acl_tag = match acl_gate.decide_full(&snap, peer.ip(), &metrics) {
                    AclGateOutcome::Admit => None,
                    AclGateOutcome::AdmitTagged(tag) => Some(tag),
                    AclGateOutcome::Drop => {
                        next_id = next_id.wrapping_add(1);
                        continue;
                    }
                    AclGateOutcome::Refuse => {
                        let (wire, _, _) =
                            build_error_response(parsed.dns_id, RCODE_REFUSED, &buf, None);
                        let len = (wire.len() as u16).to_be_bytes();
                        let _ = stream.write_all(&len);
                        let _ = stream.write_all(&wire);
                        next_id = next_id.wrapping_add(1);
                        continue;
                    }
                };

                let wire: Option<Vec<u8>> = run_in_slot(
                    &txn_store,
                    orchestrator.as_ref(),
                    &snap,
                    observation.as_ref(),
                    |slot| {
                        slot.txn = Transaction::new(next_id, peer, ClientProtocol::Tcp)
                            .with_global_query_index(global_query_index)
                            .with_listener_label(listener_label.clone())
                            .with_query_wire(buf.clone());
                        apply_parsed_query(&mut slot.txn, parsed);
                        slot.pre_parsed = true;
                        if let Some(tag) = acl_tag {
                            slot.txn.tags.set_bool(tag, true);
                        }
                        if slot.query.set_from_slice(&buf).is_err() {
                            slot.response_overflow = Some(buf.clone());
                        }
                    },
                )
                .unwrap_or_default();
                next_id = next_id.wrapping_add(1);
                if let Some(wire) = wire {
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
