//! TCP DNS listener (RFC 1035 length-prefixed).

use crate::listener::startup_log;
use conduit_core::orchestrator::{Orchestrator, RunOutcome};
use conduit_core::snapshot::SnapshotStore;
use conduit_core::transaction::{ClientProtocol, Transaction};
use conduit_events::EventHub;
use conduit_proto::config::Listener;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener};
use std::sync::Arc;
use std::time::Duration;

pub fn run_worker(
    listener: Listener,
    store: Arc<SnapshotStore>,
    orchestrator: Arc<Orchestrator>,
    observation: Arc<EventHub>,
) -> std::io::Result<()> {
    let addr: SocketAddr = listener
        .address
        .parse()
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidInput, e))?;
    let tcp = TcpListener::bind(addr)?;
    startup_log::log_listener_bound(addr, &listener.protocol);
    let mut next_id = 1u64;
    for stream in tcp.incoming() {
        let Ok(mut stream) = stream else { continue };
        let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
        let peer = stream.peer_addr()?;
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
        let mut txn = Transaction::new(next_id, peer, ClientProtocol::Tcp).with_query_wire(buf);
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
    Ok(())
}
