//! Ingress workers for split_io
//! (UDP: recv → ACL preadmission → parse → ACL full → slot;
//!  TCP: accept → ACL preadmission → read → parse → ACL full → slot).

use super::queue::{PolicyQueue, PolicyWork, ReplyRoutes, ReplyTarget};
use crate::acl_gate::{AclGate, AclGateOutcome};
use crate::listener::{tcp, udp, DataplaneShutdown};
use conduit_core::routing::listener_metric_label;
use conduit_core::snapshot::SnapshotStore;
use conduit_core::stages::send::build_error_response;
use conduit_core::structural_parse::{apply_parsed_query, structural_parse, ParsedQuery};
use conduit_core::transaction::{ClientProtocol, Transaction};
use conduit_core::txn_store::{AcquireError, SharedTxnStore};
use conduit_metrics::MetricsHub;
use conduit_proto::config::Listener;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, UdpSocket};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

/// DNS REFUSED response code.
const RCODE_REFUSED: u16 = 5;

#[allow(clippy::too_many_arguments)]
pub fn run_udp_ingress(
    udp: Arc<UdpSocket>,
    listener: Listener,
    store: Arc<SnapshotStore>,
    txn_store: SharedTxnStore,
    policy_queue: Arc<PolicyQueue>,
    reply_routes: Arc<ReplyRoutes>,
    shutdown: DataplaneShutdown,
    global_query_counter: Arc<AtomicU64>,
    metrics: Arc<MetricsHub>,
) -> std::io::Result<()> {
    udp.set_read_timeout(Some(Duration::from_secs(1)))?;
    let listener_label = listener_metric_label(&listener);
    let mut acl_gate = AclGate::new(&listener, listener_label.clone());
    let mut buf = [0u8; 4096];
    loop {
        if shutdown.is_shutdown() {
            break;
        }
        match udp.recv_from(&mut buf) {
            Ok((len, peer)) => {
                let global_query_index = global_query_counter.fetch_add(1, Ordering::Relaxed) + 1;
                let query_bytes = buf[..len].to_vec();
                let snap = store.load();
                // Tier 0: drop known-bad clients before paying for structural parse.
                if matches!(
                    acl_gate.decide_preadmission(&snap, peer.ip(), &metrics),
                    AclGateOutcome::Drop
                ) {
                    continue;
                }
                let parsed = match structural_parse(&query_bytes) {
                    Ok(parsed) => parsed,
                    Err(reason) => {
                        metrics.builtin().record_parse_rejected(reason.as_str());
                        continue;
                    }
                };
                let tag = match acl_gate.decide_full(&snap, peer.ip(), &metrics) {
                    AclGateOutcome::Admit => None,
                    AclGateOutcome::AdmitTagged(tag) => Some(tag),
                    AclGateOutcome::Drop => continue,
                    AclGateOutcome::Refuse => {
                        let (wire, _, _) = build_error_response(
                            parsed.dns_id,
                            RCODE_REFUSED,
                            &query_bytes,
                            parsed.client_udp_payload_size,
                        );
                        let _ = udp.send_to(&wire, peer);
                        continue;
                    }
                };
                let slot_id = match acquire_ingress_slot(
                    &txn_store,
                    peer,
                    ClientProtocol::Udp,
                    &listener_label,
                    global_query_index,
                    &query_bytes,
                    parsed,
                    tag,
                ) {
                    Ok(id) => id,
                    Err(AcquireError::Exhausted) => {
                        tracing::debug!("slot pool exhausted; dropping query");
                        continue;
                    }
                };
                reply_routes.insert(
                    slot_id,
                    ReplyTarget::Udp {
                        socket: udp.clone(),
                        peer,
                    },
                );
                policy_queue.push(PolicyWork::New(slot_id));
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

#[allow(clippy::too_many_arguments)]
pub fn run_tcp_ingress(
    tcp: TcpListener,
    listener: Listener,
    store: Arc<SnapshotStore>,
    txn_store: SharedTxnStore,
    policy_queue: Arc<PolicyQueue>,
    reply_routes: Arc<ReplyRoutes>,
    shutdown: DataplaneShutdown,
    global_query_counter: Arc<AtomicU64>,
    metrics: Arc<MetricsHub>,
) -> std::io::Result<()> {
    tcp.set_nonblocking(true)?;
    let listener_label = listener_metric_label(&listener);
    let mut acl_gate = AclGate::new(&listener, listener_label.clone());
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
                        metrics.builtin().record_parse_rejected(reason.as_str());
                        continue;
                    }
                };
                let tag = match acl_gate.decide_full(&snap, peer.ip(), &metrics) {
                    AclGateOutcome::Admit => None,
                    AclGateOutcome::AdmitTagged(tag) => Some(tag),
                    AclGateOutcome::Drop => continue,
                    AclGateOutcome::Refuse => {
                        let (wire, _, _) =
                            build_error_response(parsed.dns_id, RCODE_REFUSED, &buf, None);
                        let len = (wire.len() as u16).to_be_bytes();
                        let _ = stream.write_all(&len);
                        let _ = stream.write_all(&wire);
                        continue;
                    }
                };
                let slot_id = match acquire_ingress_slot(
                    &txn_store,
                    peer,
                    ClientProtocol::Tcp,
                    &listener_label,
                    global_query_index,
                    &buf,
                    parsed,
                    tag,
                ) {
                    Ok(id) => id,
                    Err(AcquireError::Exhausted) => continue,
                };
                let (reply_tx, reply_rx) = crossbeam_channel::bounded(1);
                reply_routes.insert(slot_id, ReplyTarget::Tcp { tx: reply_tx });
                policy_queue.push(PolicyWork::New(slot_id));
                if let Ok(wire) = reply_rx.recv_timeout(Duration::from_secs(30)) {
                    let len = (wire.len() as u16).to_be_bytes();
                    let _ = stream.write_all(&len);
                    let _ = stream.write_all(&wire);
                }
            }
            Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(_) if shutdown.is_shutdown() => break,
            Err(e) => return Err(e),
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn acquire_ingress_slot(
    txn_store: &SharedTxnStore,
    peer: SocketAddr,
    protocol: ClientProtocol,
    listener_label: &str,
    global_query_index: u64,
    query_bytes: &[u8],
    parsed: ParsedQuery,
    acl_tag: Option<String>,
) -> Result<conduit_core::txn_store::SlotId, AcquireError> {
    let slot_id = txn_store.acquire()?;
    let setup = txn_store.with_slot(
        slot_id,
        conduit_core::txn_store::SlotState::Ingress,
        |slot| {
            slot.txn = Transaction::new(slot_id.index() as u64, peer, protocol)
                .with_global_query_index(global_query_index)
                .with_listener_label(listener_label.to_string())
                .with_query_wire(query_bytes.to_vec());
            apply_parsed_query(&mut slot.txn, parsed);
            slot.pre_parsed = true;
            if let Some(tag) = acl_tag {
                slot.txn.tags.set_bool(tag, true);
            }
            if slot.query.set_from_slice(query_bytes).is_err() {
                slot.response_overflow = Some(query_bytes.to_vec());
            }
            Ok(())
        },
    );
    if setup.is_err() {
        let _ = txn_store.release_active(slot_id);
        return Err(AcquireError::Exhausted);
    }
    Ok(slot_id)
}

/// Bind listener sockets (reuses listener helpers).
pub fn bind_udp(
    listener: &Listener,
    reuse_port: bool,
    rcvbuf: u32,
) -> std::io::Result<(socket2::Socket, SocketAddr)> {
    udp::bind_socket(listener, reuse_port, rcvbuf)
}

pub fn bind_tcp(listener: &Listener) -> std::io::Result<(socket2::Socket, SocketAddr)> {
    tcp::bind_socket(listener)
}
