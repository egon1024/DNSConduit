//! Non-blocking upstream I/O (split_io): reply demux and timeouts.
//!
//! Under `dataplane.io_workers: N`, the process runs N independent I/O shards.
//! Each shard owns its poller, pending map, and egress UDP sockets. Forward
//! submit picks a shard by `slot_id % N` and both registers pending and sends
//! on that shard so replies land on the owning poll loop.

use crate::forward::egress::{EgressSourceSelection, WorkerForwardEgress};
use crate::forward::{ForwardKey, TxnTable};
use crate::listener::DataplaneShutdown;
use conduit_core::txn_store::SlotId;
use std::collections::HashMap;
use std::io;
use std::net::{SocketAddr, UdpSocket};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Idle poll interval when no forwards are pending (also caps wait when pending exists).
const IDLE_POLL_INTERVAL: Duration = Duration::from_millis(100);

/// Outcome of an upstream wait leg completed by the I/O backend.
#[derive(Debug, Clone)]
pub enum WaitCompletion {
    Response { wire: Vec<u8> },
    Timeout,
}

/// Resume item for the policy pool (slot id + I/O outcome).
#[derive(Debug, Clone)]
pub struct IoResume {
    pub slot_id: SlotId,
    pub completion: WaitCompletion,
}

struct PendingForward {
    slot_id: SlotId,
    deadline: Instant,
}

/// One I/O poll worker: poller, pending map, and (optionally) egress for send.
struct IoShard {
    table: Arc<TxnTable>,
    timeout: Duration,
    pending: Mutex<HashMap<ForwardKey, PendingForward>>,
    resume_tx: crossbeam_channel::Sender<IoResume>,
    poller: polling::Poller,
    sockets: Vec<UdpSocket>,
    /// Present when the shard was built from a `WorkerForwardEgress` (production
    /// `split_io` and submit-path tests). Absent for raw-socket unit tests that
    /// only exercise demux/timeout helpers.
    egress: Option<WorkerForwardEgress>,
}

/// Shared handle for registering outstanding UDP forwards across N I/O shards.
#[derive(Clone)]
pub struct IoBackend {
    shards: Arc<[IoShard]>,
}

impl IoBackend {
    /// Single-shard backend from raw UDP sockets (tests / N = 1 without egress maps).
    pub fn new(
        egress_sockets: Vec<UdpSocket>,
        table: Arc<TxnTable>,
        timeout_ms: u32,
    ) -> io::Result<(Self, crossbeam_channel::Receiver<IoResume>)> {
        let (resume_tx, resume_rx) = crossbeam_channel::unbounded();
        let shard = IoShard::from_sockets(egress_sockets, table, timeout_ms, resume_tx)?;
        Ok((
            Self {
                shards: Arc::from(vec![shard]),
            },
            resume_rx,
        ))
    }

    /// Multi-shard backend: one shard per egress set. `egresses.len()` MUST equal
    /// `dataplane.io_workers`. Failures bind/register as `Err` (no silent shrink).
    pub fn from_egresses(
        egresses: Vec<WorkerForwardEgress>,
        table: Arc<TxnTable>,
        timeout_ms: u32,
    ) -> io::Result<(Self, crossbeam_channel::Receiver<IoResume>)> {
        if egresses.is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "io_backend: at least one I/O shard egress is required",
            ));
        }
        let (resume_tx, resume_rx) = crossbeam_channel::unbounded();
        let mut shards = Vec::with_capacity(egresses.len());
        for (shard_idx, egress) in egresses.into_iter().enumerate() {
            let sockets = egress.all_udp_sockets();
            let shard =
                IoShard::from_sockets(sockets, table.clone(), timeout_ms, resume_tx.clone())
                    .map_err(|e| {
                        io::Error::new(
                            e.kind(),
                            format!("io_backend: failed to build I/O shard {shard_idx}: {e}"),
                        )
                    })?;
            shards.push(IoShard {
                egress: Some(egress),
                ..shard
            });
        }
        Ok((
            Self {
                shards: Arc::from(shards),
            },
            resume_rx,
        ))
    }

    pub fn shard_count(&self) -> usize {
        self.shards.len()
    }

    fn shard_index(&self, slot_id: SlotId) -> usize {
        (slot_id.index() as usize) % self.shards.len()
    }

    fn shard_for(&self, slot_id: SlotId) -> &IoShard {
        &self.shards[self.shard_index(slot_id)]
    }

    /// Register an outstanding UDP forward after successful send (split_io submit path).
    pub fn track_pending(&self, key: ForwardKey, slot_id: SlotId) {
        self.shard_for(slot_id).track_pending(key, slot_id);
    }

    /// Cancel tracking when submit fails after TxnTable registration.
    /// Searches all shards (error path; a wait is never pending on two shards).
    pub fn cancel_pending(&self, key: ForwardKey) {
        for shard in self.shards.iter() {
            if shard.cancel_pending(key) {
                return;
            }
        }
    }

    /// Egress UDP socket on the owning shard for this slot (sticky affinity).
    pub fn udp_socket_for<'a>(
        &'a self,
        slot_id: SlotId,
        sel: &EgressSourceSelection<'_>,
    ) -> io::Result<&'a UdpSocket> {
        let shard = self.shard_for(slot_id);
        let Some(egress) = shard.egress.as_ref() else {
            return Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "io_backend: shard has no egress map; use IoBackend::from_egresses for submit",
            ));
        };
        Ok(egress.udp_socket_for(sel))
    }

    pub fn spawn_poll_threads(
        self,
        io_stop: Arc<AtomicBool>,
        dataplane_shutdown: DataplaneShutdown,
    ) -> Vec<std::thread::JoinHandle<()>> {
        let n = self.shards.len();
        let mut handles = Vec::with_capacity(n);
        for shard_idx in 0..n {
            let backend = self.clone();
            let io_stop = io_stop.clone();
            let dataplane_shutdown = dataplane_shutdown.clone();
            handles.push(std::thread::spawn(move || {
                if let Err(e) = backend.shards[shard_idx].run_loop(&io_stop) {
                    tracing::error!(
                        shard = shard_idx,
                        error = %e,
                        "I/O backend poll loop exited; signaling dataplane shutdown"
                    );
                    dataplane_shutdown.signal();
                }
            }));
        }
        handles
    }

    #[cfg(test)]
    fn drive_timeouts(&self) -> io::Result<()> {
        for shard in self.shards.iter() {
            shard.poll_timeouts()?;
        }
        Ok(())
    }

    #[cfg(test)]
    fn handle_reply(&self, from: SocketAddr, wire: &[u8]) -> io::Result<()> {
        // Unit tests inject replies without a live poller; try each shard so
        // multi-shard demux tests can assert ownership without socket I/O.
        for shard in self.shards.iter() {
            if shard.pending.lock().unwrap().contains_key(&ForwardKey {
                backend: from,
                dns_id: if wire.len() >= 2 {
                    u16::from_be_bytes([wire[0], wire[1]])
                } else {
                    0
                },
            }) {
                return shard.handle_reply(from, wire);
            }
        }
        if let Some(shard) = self.shards.first() {
            return shard.handle_reply(from, wire);
        }
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn spawn_poll_thread_for_test(
        self,
        io_stop: Arc<AtomicBool>,
    ) -> std::thread::JoinHandle<()> {
        let n = self.shards.len();
        let stop = io_stop.clone();
        let backend = self.clone();
        std::thread::spawn(move || {
            let mut joins = Vec::with_capacity(n);
            for shard_idx in 0..n {
                let backend = backend.clone();
                let stop = stop.clone();
                joins.push(std::thread::spawn(move || {
                    if let Err(e) = backend.shards[shard_idx].run_loop(&stop) {
                        tracing::error!(
                            shard = shard_idx,
                            error = %e,
                            "I/O backend poll loop exited (test)"
                        );
                    }
                }));
            }
            for j in joins {
                let _ = j.join();
            }
        })
    }
}

impl IoShard {
    fn from_sockets(
        egress_sockets: Vec<UdpSocket>,
        table: Arc<TxnTable>,
        timeout_ms: u32,
        resume_tx: crossbeam_channel::Sender<IoResume>,
    ) -> io::Result<Self> {
        let poller = polling::Poller::new()?;
        let mut sockets = Vec::with_capacity(egress_sockets.len());
        for sock in egress_sockets {
            // Egress sockets may carry SO_RCVTIMEO from bind; epoll + nonblocking recv does not need it.
            sock.set_read_timeout(None)?;
            sock.set_nonblocking(true)?;
            sockets.push(sock);
        }
        for (idx, sock) in sockets.iter().enumerate() {
            // SAFETY: sockets outlive the poller and are not moved after registration.
            unsafe {
                poller.add_with_mode(
                    sock,
                    polling::Event::readable(idx),
                    polling::PollMode::Level,
                )?;
            }
        }
        Ok(Self {
            table,
            timeout: Duration::from_millis(timeout_ms as u64),
            pending: Mutex::new(HashMap::new()),
            resume_tx,
            poller,
            sockets,
            egress: None,
        })
    }

    fn track_pending(&self, key: ForwardKey, slot_id: SlotId) {
        let deadline = Instant::now() + self.timeout;
        self.pending
            .lock()
            .unwrap()
            .insert(key, PendingForward { slot_id, deadline });
    }

    /// Returns true if the key was present on this shard.
    fn cancel_pending(&self, key: ForwardKey) -> bool {
        self.pending.lock().unwrap().remove(&key).is_some()
    }

    fn run_loop(&self, shutdown: &AtomicBool) -> io::Result<()> {
        let mut events = polling::Events::new();
        loop {
            if shutdown.load(Ordering::Relaxed) {
                break;
            }
            self.poll_timeouts()
                .map_err(|e| io_err("poll_timeouts", e))?;

            let wait = self
                .next_wait_timeout()
                .map(|d| d.min(IDLE_POLL_INTERVAL))
                .or(Some(IDLE_POLL_INTERVAL));

            // polling::Poller::wait appends; must clear before each wait.
            events.clear();
            self.poller
                .wait(&mut events, wait)
                .map_err(|e| io_err("poller.wait", e))?;

            for ev in events.iter() {
                if ev.readable {
                    self.on_readable(ev.key)
                        .map_err(|e| io_err(&format!("on_readable(token={})", ev.key), e))?;
                } else if ev.is_err() == Some(true) {
                    tracing::warn!(
                        token = ev.key,
                        "io_backend: poll event with error flag (ignored)"
                    );
                }
            }
        }
        Ok(())
    }

    fn next_wait_timeout(&self) -> Option<Duration> {
        let pending = self.pending.lock().unwrap();
        let now = Instant::now();
        let next = pending
            .values()
            .map(|p| p.deadline.saturating_duration_since(now))
            .min();
        next.map(|d| d.min(IDLE_POLL_INTERVAL))
    }

    fn poll_timeouts(&self) -> io::Result<()> {
        let now = Instant::now();
        let expired: Vec<ForwardKey> = {
            let pending = self.pending.lock().unwrap();
            pending
                .iter()
                .filter(|(_, p)| p.deadline <= now)
                .map(|(k, _)| *k)
                .collect()
        };
        for key in expired {
            self.complete(key, WaitCompletion::Timeout)?;
        }
        Ok(())
    }

    fn on_readable(&self, token: usize) -> io::Result<()> {
        let mut buf = [0u8; 4096];
        let Some(sock) = self.sockets.get(token) else {
            tracing::debug!(token, "io_backend: readable event for unknown socket token");
            return Ok(());
        };
        loop {
            match sock.recv_from(&mut buf) {
                Ok((len, from)) => self.handle_reply(from, &buf[..len])?,
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => break,
                Err(e) => return Err(e),
            }
        }
        Ok(())
    }

    fn handle_reply(&self, from: SocketAddr, wire: &[u8]) -> io::Result<()> {
        if wire.len() < 12 {
            return Ok(());
        }
        let dns_id = u16::from_be_bytes([wire[0], wire[1]]);
        let key = ForwardKey {
            backend: from,
            dns_id,
        };
        if !self.pending.lock().unwrap().contains_key(&key) {
            tracing::debug!(%from, dns_id, "io_backend: unmatched reply");
            return Ok(());
        }
        self.complete(
            key,
            WaitCompletion::Response {
                wire: wire.to_vec(),
            },
        )
    }

    fn complete(&self, key: ForwardKey, completion: WaitCompletion) -> io::Result<()> {
        let Some(pending) = self.pending.lock().unwrap().remove(&key) else {
            return Ok(());
        };
        self.table.remove(key);
        // Forward attempt metrics (success and timeout) are recorded on the
        // policy worker in `WaitResponseStage`, where the transaction's selected
        // pool/backend are available for correct, name-resolved labels.
        let _ = self.resume_tx.send(IoResume {
            slot_id: pending.slot_id,
            completion,
        });
        Ok(())
    }
}

fn io_err(op: &str, e: io::Error) -> io::Error {
    io::Error::new(e.kind(), format!("io_backend {op}: {e}"))
}

/// Apply an I/O completion to a transaction before resuming the orchestrator.
pub fn apply_wait_completion(txn: &mut conduit_core::Transaction, completion: &WaitCompletion) {
    match completion {
        WaitCompletion::Response { wire } => {
            txn.complete_forward_rtt_from_mark();
            txn.response_wire = Some(wire.clone());
        }
        WaitCompletion::Timeout => {
            txn.complete_forward_rtt_from_mark();
            txn.set_rcode_name("SERVFAIL");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use conduit_config::forward::{CompiledForward, UpstreamTransport};
    use conduit_core::transaction::{ClientProtocol, Transaction};
    use std::net::Ipv4Addr;
    use std::thread;
    use std::time::Duration;

    fn example_query() -> Vec<u8> {
        vec![
            0xab, 0xcd, 0x01, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x07, 0x65,
            0x78, 0x61, 0x6d, 0x70, 0x6c, 0x65, 0x03, 0x63, 0x6f, 0x6d, 0x00, 0x00, 0x01, 0x00,
            0x01,
        ]
    }

    fn example_response() -> Vec<u8> {
        let mut wire = example_query();
        wire[2] = 0x81;
        wire[3] = 0x80;
        wire
    }

    fn compiled_forward() -> CompiledForward {
        CompiledForward {
            sources_v4: vec![],
            sources_v6: vec![],
            source_selection: "round_robin".into(),
            upstream_transport: UpstreamTransport::UdpOnly,
            client_tcp_uses_upstream_tcp: false,
            timeout_ms: 1000,
            outstanding_per_backend: 10,
        }
    }

    #[test]
    fn delayed_reply_demuxes_by_dns_id() {
        let table = Arc::new(TxnTable::new(64, 32));
        let egress = UdpSocket::bind("127.0.0.1:0").unwrap();
        let (io, resume_rx) = IoBackend::new(vec![egress], table.clone(), 2000).unwrap();

        let backend: SocketAddr = "127.0.0.1:5301".parse().unwrap();
        let key = ForwardKey {
            backend,
            dns_id: 0xabcd,
        };
        assert!(table.register(key, 7));
        io.track_pending(key, SlotId::from_index(7));

        io.handle_reply(backend, &example_response()).unwrap();

        let resume = resume_rx
            .recv_timeout(Duration::from_secs(1))
            .expect("resume within timeout");
        assert_eq!(resume.slot_id, SlotId::from_index(7));
        match resume.completion {
            WaitCompletion::Response { wire } => assert_eq!(wire, example_response()),
            WaitCompletion::Timeout => panic!("expected response"),
        }
        assert!(table.lookup(key).is_none());
    }

    #[test]
    fn unmatched_dns_id_is_ignored() {
        let table = Arc::new(TxnTable::new(64, 32));
        let egress = UdpSocket::bind("127.0.0.1:0").unwrap();
        let (io, resume_rx) = IoBackend::new(vec![egress], table.clone(), 2000).unwrap();
        let backend: SocketAddr = "127.0.0.1:5302".parse().unwrap();
        let key = ForwardKey {
            backend,
            dns_id: 0xabcd,
        };
        assert!(table.register(key, 7));
        io.track_pending(key, SlotId::from_index(7));

        let mut other = example_response();
        other[0] = 0x00;
        other[1] = 0x01;
        io.handle_reply(backend, &other).unwrap();

        assert!(resume_rx.try_recv().is_err());
        assert!(table.lookup(key).is_some());
    }

    #[test]
    fn poll_loop_survives_many_idle_waits() {
        let table = Arc::new(TxnTable::new(64, 32));
        let egress = UdpSocket::bind("127.0.0.1:0").unwrap();
        let (io, _resume_rx) = IoBackend::new(vec![egress], table, 2000).unwrap();
        let stop = Arc::new(AtomicBool::new(false));
        let handle = io.spawn_poll_thread_for_test(stop.clone());
        thread::sleep(Duration::from_secs(3));
        assert!(!stop.load(Ordering::Relaxed));
        stop.store(true, Ordering::Relaxed);
        handle.join().expect("poll thread join");
    }

    #[test]
    fn timeout_emits_resume_without_wire() {
        let table = Arc::new(TxnTable::new(64, 32));
        let egress = UdpSocket::bind("127.0.0.1:0").unwrap();
        let (io, resume_rx) = IoBackend::new(vec![egress], table.clone(), 50).unwrap();

        let backend: SocketAddr = "127.0.0.1:9".parse().unwrap();
        let key = ForwardKey {
            backend,
            dns_id: 42,
        };
        assert!(table.register(key, 3));
        io.track_pending(key, SlotId::from_index(3));

        thread::sleep(Duration::from_millis(60));
        io.drive_timeouts().unwrap();

        let resume = resume_rx
            .recv_timeout(Duration::from_secs(1))
            .expect("timeout resume");
        assert_eq!(resume.slot_id, SlotId::from_index(3));
        assert!(matches!(resume.completion, WaitCompletion::Timeout));
        assert!(table.lookup(key).is_none());

        let mut txn = Transaction::new(3, "127.0.0.1:53".parse().unwrap(), ClientProtocol::Udp);
        txn.mark_forward_started(Instant::now() - Duration::from_millis(60));
        apply_wait_completion(&mut txn, &resume.completion);
        assert_eq!(txn.rcode_label().as_deref(), Some("SERVFAIL"));
        assert!(txn.last_forward_ms() >= 50);
    }

    #[test]
    fn multi_shard_demux_resumes_owning_slot() {
        let table = Arc::new(TxnTable::new(64, 32));
        let compiled = compiled_forward();
        let e0 = WorkerForwardEgress::new(&compiled, &[Ipv4Addr::UNSPECIFIED], &[], 2000).unwrap();
        let e1 = WorkerForwardEgress::new(&compiled, &[Ipv4Addr::UNSPECIFIED], &[], 2000).unwrap();
        let (io, resume_rx) = IoBackend::from_egresses(vec![e0, e1], table.clone(), 2000).unwrap();
        assert_eq!(io.shard_count(), 2);

        let backend: SocketAddr = "127.0.0.1:5303".parse().unwrap();
        // slot 7 → shard 1 (7 % 2); slot 8 → shard 0
        let key7 = ForwardKey {
            backend,
            dns_id: 0xabcd,
        };
        let key8 = ForwardKey {
            backend,
            dns_id: 0xabce,
        };
        assert!(table.register(key7, 7));
        assert!(table.register(key8, 8));
        io.track_pending(key7, SlotId::from_index(7));
        io.track_pending(key8, SlotId::from_index(8));

        let mut resp7 = example_response();
        resp7[0] = 0xab;
        resp7[1] = 0xcd;
        io.shards[1].handle_reply(backend, &resp7).unwrap();

        let resume = resume_rx
            .recv_timeout(Duration::from_secs(1))
            .expect("shard-1 resume");
        assert_eq!(resume.slot_id, SlotId::from_index(7));
        assert!(matches!(resume.completion, WaitCompletion::Response { .. }));

        // Wrong shard must not complete key8
        io.shards[1]
            .handle_reply(
                backend,
                &[
                    0xab, 0xce, 0x81, 0x80, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
                ],
            )
            .unwrap();
        assert!(resume_rx.try_recv().is_err());
        assert!(table.lookup(key8).is_some());

        io.shards[0]
            .handle_reply(
                backend,
                &[
                    0xab, 0xce, 0x81, 0x80, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
                ],
            )
            .unwrap();
        let resume8 = resume_rx
            .recv_timeout(Duration::from_secs(1))
            .expect("shard-0 resume");
        assert_eq!(resume8.slot_id, SlotId::from_index(8));
    }

    #[test]
    fn multi_shard_timeout_completes_once_on_owning_shard() {
        let table = Arc::new(TxnTable::new(64, 32));
        let compiled = compiled_forward();
        let e0 = WorkerForwardEgress::new(&compiled, &[Ipv4Addr::UNSPECIFIED], &[], 50).unwrap();
        let e1 = WorkerForwardEgress::new(&compiled, &[Ipv4Addr::UNSPECIFIED], &[], 50).unwrap();
        let (io, resume_rx) = IoBackend::from_egresses(vec![e0, e1], table.clone(), 50).unwrap();

        let backend: SocketAddr = "127.0.0.1:9".parse().unwrap();
        let key = ForwardKey {
            backend,
            dns_id: 99,
        };
        // slot 5 → shard 1
        assert!(table.register(key, 5));
        io.track_pending(key, SlotId::from_index(5));

        thread::sleep(Duration::from_millis(60));
        io.drive_timeouts().unwrap();

        let resume = resume_rx
            .recv_timeout(Duration::from_secs(1))
            .expect("one timeout resume");
        assert_eq!(resume.slot_id, SlotId::from_index(5));
        assert!(matches!(resume.completion, WaitCompletion::Timeout));
        assert!(resume_rx.try_recv().is_err(), "no double-complete");
        assert!(table.lookup(key).is_none());
    }

    #[test]
    fn from_egresses_rejects_empty() {
        let table = Arc::new(TxnTable::new(8, 4));
        let err = match IoBackend::from_egresses(vec![], table, 1000) {
            Ok(_) => panic!("expected empty egress list to fail"),
            Err(e) => e,
        };
        assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
    }
}
