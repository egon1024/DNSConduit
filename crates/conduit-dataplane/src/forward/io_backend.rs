//! Non-blocking upstream I/O (split_io): reply demux and timeouts.

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

/// Shared handle for registering outstanding UDP forwards.
#[derive(Clone)]
pub struct IoBackend {
    inner: Arc<IoBackendInner>,
}

struct IoBackendInner {
    table: Arc<TxnTable>,
    timeout: Duration,
    pending: Mutex<HashMap<ForwardKey, PendingForward>>,
    resume_tx: crossbeam_channel::Sender<IoResume>,
    poller: polling::Poller,
    sockets: Vec<UdpSocket>,
}

impl IoBackend {
    pub fn new(
        egress_sockets: Vec<UdpSocket>,
        table: Arc<TxnTable>,
        timeout_ms: u32,
    ) -> io::Result<(Self, crossbeam_channel::Receiver<IoResume>)> {
        let (resume_tx, resume_rx) = crossbeam_channel::unbounded();
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
        let inner = Arc::new(IoBackendInner {
            table,
            timeout: Duration::from_millis(timeout_ms as u64),
            pending: Mutex::new(HashMap::new()),
            resume_tx,
            poller,
            sockets,
        });
        Ok((Self { inner }, resume_rx))
    }

    /// Register an outstanding UDP forward after successful send (split_io submit path).
    pub fn track_pending(&self, key: ForwardKey, slot_id: SlotId) {
        let deadline = Instant::now() + self.inner.timeout;
        self.inner
            .pending
            .lock()
            .unwrap()
            .insert(key, PendingForward { slot_id, deadline });
    }

    /// Cancel tracking when submit fails after TxnTable registration.
    pub fn cancel_pending(&self, key: ForwardKey) {
        self.inner.pending.lock().unwrap().remove(&key);
    }

    pub fn spawn_poll_thread(
        self,
        io_stop: Arc<AtomicBool>,
        dataplane_shutdown: DataplaneShutdown,
    ) -> std::thread::JoinHandle<()> {
        std::thread::spawn(move || {
            if let Err(e) = self.run_loop(&io_stop) {
                tracing::error!(
                    error = %e,
                    "I/O backend poll loop exited; signaling dataplane shutdown"
                );
                dataplane_shutdown.signal();
            }
        })
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
            self.inner
                .poller
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
        let pending = self.inner.pending.lock().unwrap();
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
            let pending = self.inner.pending.lock().unwrap();
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
        let Some(sock) = self.inner.sockets.get(token) else {
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
        if !self.inner.pending.lock().unwrap().contains_key(&key) {
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
        let Some(pending) = self.inner.pending.lock().unwrap().remove(&key) else {
            return Ok(());
        };
        self.inner.table.remove(key);
        // Forward attempt metrics (success and timeout) are recorded on the
        // policy worker in `WaitResponseStage`, where the transaction's selected
        // pool/backend are available for correct, name-resolved labels.
        let _ = self.inner.resume_tx.send(IoResume {
            slot_id: pending.slot_id,
            completion,
        });
        Ok(())
    }

    #[cfg(test)]
    fn drive_timeouts(&self) -> io::Result<()> {
        self.poll_timeouts()
    }

    #[cfg(test)]
    fn spawn_poll_thread_for_test(self, io_stop: Arc<AtomicBool>) -> std::thread::JoinHandle<()> {
        std::thread::spawn(move || {
            if let Err(e) = self.run_loop(&io_stop) {
                tracing::error!(error = %e, "I/O backend poll loop exited (test)");
            }
        })
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
    use conduit_core::transaction::{ClientProtocol, Transaction};
    use std::net::UdpSocket;
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
}
