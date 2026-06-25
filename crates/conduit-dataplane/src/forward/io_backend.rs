//! Non-blocking upstream I/O (split_io): reply demux and timeouts.

use crate::forward::{ForwardKey, TxnTable};
use conduit_core::txn_store::SlotId;
use conduit_metrics::MetricsHub;
use std::collections::HashMap;
use std::io;
use std::net::{SocketAddr, UdpSocket};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

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
    metrics: Option<Arc<MetricsHub>>,
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
        metrics: Option<Arc<MetricsHub>>,
    ) -> io::Result<(Self, crossbeam_channel::Receiver<IoResume>)> {
        let (resume_tx, resume_rx) = crossbeam_channel::unbounded();
        let poller = polling::Poller::new()?;
        let mut sockets = Vec::with_capacity(egress_sockets.len());
        for sock in egress_sockets {
            sock.set_nonblocking(true)?;
            sockets.push(sock);
        }
        for (idx, sock) in sockets.iter().enumerate() {
            // SAFETY: sockets outlive the poller and are not moved after registration.
            unsafe {
                poller.add(sock, polling::Event::readable(idx))?;
            }
        }
        let inner = Arc::new(IoBackendInner {
            table,
            timeout: Duration::from_millis(timeout_ms as u64),
            metrics,
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

    pub fn spawn_poll_thread(self, shutdown: Arc<AtomicBool>) -> std::thread::JoinHandle<()> {
        std::thread::spawn(move || {
            if let Err(e) = self.run_loop(&shutdown) {
                tracing::error!(error = %e, "I/O backend poll loop exited");
            }
        })
    }

    fn run_loop(&self, shutdown: &AtomicBool) -> io::Result<()> {
        let mut events = polling::Events::new();
        loop {
            if shutdown.load(Ordering::Relaxed) {
                break;
            }
            self.poll_timeouts()?;
            let wait = self
                .next_wait_timeout()
                .map(|d| d.min(Duration::from_millis(100)))
                .or(Some(Duration::from_millis(100)));
            self.inner.poller.wait(&mut events, wait)?;
            for ev in events.iter() {
                if ev.readable {
                    self.on_readable(ev.key)?;
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
        next.map(|d| d.min(Duration::from_millis(100)))
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
            return Ok(());
        };
        let (len, from) = match sock.recv_from(&mut buf) {
            Ok(r) => r,
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => return Ok(()),
            Err(e) => return Err(e),
        };
        self.handle_reply(from, &buf[..len])
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
        if matches!(completion, WaitCompletion::Timeout) {
            self.record_timeout_metrics(key.backend);
        }
        let _ = self.inner.resume_tx.send(IoResume {
            slot_id: pending.slot_id,
            completion,
        });
        Ok(())
    }

    fn record_timeout_metrics(&self, backend: SocketAddr) {
        let Some(hub) = self.inner.metrics.as_ref() else {
            return;
        };
        if !hub.metrics_enabled() {
            return;
        }
        let backend_label = backend.to_string();
        hub.builtin
            .record_forward_attempt("unknown", &backend_label, "error");
        hub.builtin.record_forward_error("unknown", "timeout");
    }

    #[cfg(test)]
    fn drive_timeouts(&self) -> io::Result<()> {
        self.poll_timeouts()
    }
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
        let (io, resume_rx) = IoBackend::new(vec![egress], table.clone(), 2000, None).unwrap();

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
        let (io, resume_rx) = IoBackend::new(vec![egress], table.clone(), 2000, None).unwrap();
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
    fn timeout_emits_resume_without_wire() {
        let table = Arc::new(TxnTable::new(64, 32));
        let egress = UdpSocket::bind("127.0.0.1:0").unwrap();
        let (io, resume_rx) = IoBackend::new(vec![egress], table.clone(), 50, None).unwrap();

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
