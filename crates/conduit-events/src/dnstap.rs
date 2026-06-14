//! Dnstap protobuf + framestream writer for one sink instance.

use crate::compile::{CompiledSinkInstance, Destination};
use crate::connect_retry::BackoffState;
use crate::event::{EventKind, ExportEvent};
use crate::fstrm;
use crate::sink::EventSink;
use crossbeam_channel::{Receiver, RecvTimeoutError};
use dnstap::{ClientQuery, ClientResponse, DNSMessage, SocketFamily, SocketProtocol};
use protobuf::Message;
use std::io::{self, Read, Write};
use std::net::TcpStream;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

const CONTENT_TYPE: &str = "protobuf:dnstap.Dnstap";
const RECV_TIMEOUT: Duration = Duration::from_millis(250);
/// Throttle "still failing" retry warnings while a collector outage continues.
const RETRY_WARN_INTERVAL: Duration = Duration::from_secs(60);

/// One-shot disconnect/unreachable warn plus throttled retry warnings per outage.
struct ConnectLogState {
    outage_logged: bool,
    last_retry_warn: Option<std::time::Instant>,
}

impl ConnectLogState {
    fn new() -> Self {
        Self {
            outage_logged: false,
            last_retry_warn: None,
        }
    }

    fn mark_disconnected(&mut self, sink: &str) {
        if self.outage_logged {
            return;
        }
        tracing::warn!(
            sink = %sink,
            "dnstap destinations disconnected, reconnecting"
        );
        self.outage_logged = true;
        // Suppress an immediate duplicate "still failing" on the same retry tick.
        self.last_retry_warn = Some(std::time::Instant::now());
    }

    fn mark_unreachable(&mut self, sink: &str) {
        if self.outage_logged {
            return;
        }
        tracing::warn!(
            sink = %sink,
            "dnstap destinations unreachable, reconnecting"
        );
        self.outage_logged = true;
        self.last_retry_warn = Some(std::time::Instant::now());
    }

    fn on_retry_sleep(&mut self, sink: &str, connect_attempts: u64, delay: Duration) {
        tracing::debug!(
            sink = %sink,
            connect_attempts,
            delay_ms = delay.as_millis(),
            "dnstap connect retry sleep"
        );
        let now = std::time::Instant::now();
        let due = self
            .last_retry_warn
            .map(|t| now.duration_since(t) >= RETRY_WARN_INTERVAL)
            .unwrap_or(true);
        if due && self.outage_logged {
            tracing::warn!(
                sink = %sink,
                connect_attempts,
                delay_ms = delay.as_millis(),
                "dnstap connect retry still failing"
            );
            self.last_retry_warn = Some(now);
        }
    }

    fn on_connected(&mut self, sink: &str, destinations: usize) {
        if self.outage_logged {
            tracing::info!(
                sink = %sink,
                destinations,
                "dnstap connected"
            );
        }
        self.outage_logged = false;
        self.last_retry_warn = None;
    }
}

pub struct DnstapSink {
    instance: CompiledSinkInstance,
}

impl DnstapSink {
    pub fn new(instance: CompiledSinkInstance) -> Self {
        Self { instance }
    }

    pub fn run_with_shutdown(self, rx: Receiver<ExportEvent>, shutdown: Arc<AtomicBool>) {
        self.run_inner(rx, Some(shutdown));
    }
}

impl EventSink for DnstapSink {
    fn run(self, rx: Receiver<ExportEvent>) {
        self.run_inner(rx, None);
    }
}

impl DnstapSink {
    fn run_inner(self, rx: Receiver<ExportEvent>, shutdown: Option<Arc<AtomicBool>>) {
        let identity = self.instance.export_id.as_bytes().to_vec();
        let metrics = self.instance.metrics.clone();
        let mut writers = Vec::new();
        let mut backoff = BackoffState::new(self.instance.connect_retry.clone());
        let mut connect_log = ConnectLogState::new();
        let sink = self.instance.name.as_str();

        loop {
            if shutdown
                .as_ref()
                .is_some_and(|flag| flag.load(Ordering::Relaxed))
            {
                break;
            }
            if writers.is_empty() {
                metrics.set_connected(false);
                metrics.record_connect_attempt();
                writers = connect_all(&self.instance.destinations);
                if writers.is_empty() {
                    if !connect_log.outage_logged {
                        connect_log.mark_unreachable(sink);
                    }
                    let delay = backoff.next_delay();
                    let attempts = metrics.snapshot().connect_attempts;
                    connect_log.on_retry_sleep(sink, attempts, delay);
                    if !sleep_with_shutdown(shutdown.as_ref(), delay) {
                        break;
                    }
                    continue;
                }
                backoff.reset();
                metrics.set_connected(true);
                connect_log.on_connected(sink, writers.len());
            }

            match rx.recv_timeout(RECV_TIMEOUT) {
                Ok(event) => {
                    let bytes = match encode_event(&identity, &event) {
                        Ok(b) => b,
                        Err(e) => {
                            metrics.record_encode_failed();
                            tracing::warn!(
                                sink = %self.instance.name,
                                error = %e,
                                "dnstap encode failed"
                            );
                            continue;
                        }
                    };
                    let before = writers.len();
                    writers.retain_mut(|w| match w.write_frame(&bytes) {
                        Ok(()) => {
                            metrics.record_delivered();
                            true
                        }
                        Err(_) => {
                            metrics.record_write_failed();
                            false
                        }
                    });
                    if before > 0 && writers.is_empty() {
                        metrics.set_connected(false);
                        connect_log.mark_disconnected(sink);
                    }
                }
                Err(RecvTimeoutError::Timeout) => continue,
                Err(RecvTimeoutError::Disconnected) => break,
            }
        }
        metrics.set_connected(false);
        for w in writers {
            let _ = w.finish();
        }
    }
}

fn sleep_with_shutdown(shutdown: Option<&Arc<AtomicBool>>, delay: Duration) -> bool {
    let step = Duration::from_millis(50);
    let mut remaining = delay;
    while remaining > Duration::ZERO {
        if shutdown.is_some_and(|flag| flag.load(Ordering::Relaxed)) {
            return false;
        }
        let sleep = remaining.min(step);
        std::thread::sleep(sleep);
        remaining = remaining.saturating_sub(sleep);
    }
    true
}

trait ReadWrite: Read + Write + Send {}
impl<T: Read + Write + Send> ReadWrite for T {}

struct FrameWriter {
    inner: fstrm::FrameStreamWriter<Box<dyn ReadWrite>>,
}

impl FrameWriter {
    fn write_frame(&mut self, frame: &[u8]) -> io::Result<()> {
        self.inner.write_data_frame(frame)
    }

    fn finish(self) -> io::Result<()> {
        self.inner.finish()
    }
}

fn connect_all(destinations: &[Destination]) -> Vec<FrameWriter> {
    destinations
        .iter()
        .filter_map(|d| connect_one(d).ok())
        .collect()
}

fn connect_one(dest: &Destination) -> io::Result<FrameWriter> {
    let stream: Box<dyn ReadWrite> = match dest {
        Destination::Unix(path) => Box::new(unix_connect(path)?),
        Destination::Tcp { host, port } => {
            let addr = format!("{host}:{port}");
            let stream = TcpStream::connect(addr)?;
            stream.set_nodelay(true)?;
            Box::new(stream)
        }
    };
    let inner = fstrm::connect_bidirectional(stream, CONTENT_TYPE)?;
    Ok(FrameWriter { inner })
}

#[cfg(unix)]
fn unix_connect(path: &Path) -> io::Result<std::os::unix::net::UnixStream> {
    use std::os::unix::net::UnixStream;
    UnixStream::connect(path)
}

#[cfg(not(unix))]
fn unix_connect(_path: &Path) -> io::Result<Box<dyn ReadWrite>> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "unix dnstap destinations require unix",
    ))
}

fn encode_event(identity: &[u8], event: &ExportEvent) -> io::Result<Vec<u8>> {
    let now = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default();
    let (family, protocol) = socket_meta(event.client_addr, event.protocol_udp);
    let mut dns: DNSMessage = match event.kind {
        EventKind::Query | EventKind::Retry => ClientQuery {
            identity: Some(identity.to_vec()),
            version: None,
            socket_family: family,
            socket_protocol: protocol,
            query_time: now,
            query_packet: event.wire.clone(),
        }
        .into(),
        EventKind::Response => ClientResponse {
            identity: Some(identity.to_vec()),
            version: None,
            socket_family: family,
            socket_protocol: protocol,
            response_time: now,
            response_packet: event.wire.clone(),
        }
        .into(),
    };
    match event.kind {
        EventKind::Query | EventKind::Retry => {
            dns.query_address = Some(event.client_addr.ip());
            dns.query_port = Some(event.client_addr.port());
        }
        EventKind::Response => {
            dns.query_address = Some(event.client_addr.ip());
            dns.query_port = Some(event.client_addr.port());
            dns.response_address = Some(event.client_addr.ip());
            dns.response_port = Some(event.client_addr.port());
        }
    }
    let mut pb = dns.into_protobuf();
    if let Some(ref extra) = event.extra {
        pb.set_extra(extra.clone());
    }
    if matches!(event.kind, EventKind::Response) {
        if let Some(msg) = pb.message.as_mut() {
            if msg.has_query_message() {
                let wire = msg.take_query_message();
                msg.set_response_message(wire);
            }
        }
    }
    pb.write_to_bytes()
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
}

fn socket_meta(addr: std::net::SocketAddr, udp: bool) -> (SocketFamily, SocketProtocol) {
    let family = match addr.ip() {
        std::net::IpAddr::V4(_) => SocketFamily::INET,
        std::net::IpAddr::V6(_) => SocketFamily::INET6,
    };
    let protocol = if udp {
        SocketProtocol::UDP
    } else {
        SocketProtocol::TCP
    };
    (family, protocol)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compile::{compile_one_sink, parse_connect_retry};
    use crate::connect_retry::ConnectRetryConfig;
    use crate::event::{EventKind, ExportEvent};
    use crate::sink::EventSink;
    use conduit_proto::config::{ConnectRetry, EventSink as EventSinkConfig};
    use std::net::{IpAddr, Ipv4Addr, SocketAddr};
    use std::thread;
    use std::time::Duration;

    fn dnstap_sink(
        export_id: &str,
        destinations: Vec<&str>,
        connect_retry: Option<ConnectRetry>,
    ) -> CompiledSinkInstance {
        let sink = EventSinkConfig {
            r#type: "dnstap".into(),
            export_id: export_id.into(),
            destinations: destinations.into_iter().map(String::from).collect(),
            emit: vec!["query".into()],
            filters: None,
            extra_fields: vec![],
            extra_tags: vec![],
            name: None,
            connect_retry,
        };
        compile_one_sink(&sink, None).unwrap()
    }

    #[test]
    fn connect_log_disconnect_and_unreachable_are_one_shot_per_outage() {
        let mut log = ConnectLogState::new();
        log.mark_disconnected("tap-a");
        assert!(log.outage_logged);
        log.mark_disconnected("tap-a");
        log.mark_unreachable("tap-a");
        assert!(log.outage_logged);
        log.on_connected("tap-a", 1);
        assert!(!log.outage_logged);
        log.mark_unreachable("tap-a");
        assert!(log.outage_logged);
    }

    #[test]
    fn encode_client_query_bytes() {
        let event = ExportEvent {
            kind: EventKind::Query,
            txn_id: 1,
            client_addr: SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 1234),
            protocol_udp: true,
            wire: vec![0x00, 0x01],
            attempt_count: 1,
            extra: None,
        };
        let bytes = encode_event(b"conduit", &event).unwrap();
        assert!(!bytes.is_empty());
    }

    #[test]
    fn encode_includes_extra_in_dnstap_protobuf() {
        let extra_json = br#"{"pool":"default","backend":"192.168.1.21:53","attempt_count":1}"#;
        let event = ExportEvent {
            kind: EventKind::Response,
            txn_id: 1,
            client_addr: SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 1234),
            protocol_udp: true,
            wire: vec![0x00, 0x01, 0x81, 0x80, 0x00, 0x01],
            attempt_count: 1,
            extra: Some(extra_json.to_vec()),
        };
        let bytes = encode_event(b"conduit-dev", &event).unwrap();
        assert!(
            bytes.windows(extra_json.len()).any(|w| w == extra_json),
            "encoded dnstap protobuf should embed extra JSON bytes"
        );
    }

    #[cfg(unix)]
    #[test]
    fn writes_data_through_real_fstrm_capture() {
        use std::process::{Command, Stdio};

        if Command::new("fstrm_capture").arg("-h").output().is_err() {
            eprintln!("skipping: fstrm_capture not installed");
            return;
        }

        let dir = tempfile::tempdir().unwrap();
        let sock = dir.path().join("cap.sock");
        let out = dir.path().join("out.fstrm");

        let mut cap = Command::new("fstrm_capture")
            .args([
                "-dddd",
                "-t",
                "protobuf:dnstap.Dnstap",
                "-u",
                sock.to_str().unwrap(),
                "-w",
                out.to_str().unwrap(),
            ])
            .stderr(Stdio::piped())
            .spawn()
            .expect("spawn fstrm_capture");

        thread::sleep(Duration::from_millis(300));
        assert!(
            sock.exists(),
            "fstrm_capture did not create socket at {}",
            sock.display()
        );
        assert!(
            cap.try_wait().unwrap().is_none(),
            "fstrm_capture exited before test connect"
        );

        let dest = Destination::Unix(sock);
        let mut writer = connect_one(&dest).expect("connect");
        let event = ExportEvent {
            kind: EventKind::Response,
            txn_id: 1,
            client_addr: SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 1234),
            protocol_udp: true,
            wire: vec![0x00, 0x01, 0x81, 0x80, 0x00, 0x01],
            attempt_count: 1,
            extra: Some(br#"{"pool":"default"}"#.to_vec()),
        };
        writer
            .write_frame(b"test-payload-12345")
            .expect("write raw data frame");
        let bytes = encode_event(b"conduit-dev", &event).unwrap();
        assert!(!bytes.is_empty());
        writer.write_frame(&bytes).expect("write dnstap data frame");

        writer.finish().expect("finish stream");
        let _ = Command::new("kill")
            .args(["-HUP", &cap.id().to_string()])
            .status();
        thread::sleep(Duration::from_millis(100));
        cap.kill().ok();
        let output = cap.wait_with_output().expect("wait fstrm_capture");
        let stderr = String::from_utf8_lossy(&output.stderr);
        eprintln!("fstrm_capture stderr:\n{stderr}");

        let len = std::fs::metadata(&out).unwrap().len();
        assert!(
            len > 42,
            "expected data in capture file, got {len} bytes; fstrm_capture stderr:\n{stderr}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn sink_connects_with_bidirectional_handshake() {
        use std::os::unix::net::UnixListener;
        use std::sync::mpsc;

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sink.sock");
        let listener = UnixListener::bind(&path).unwrap();
        let (tx, rx_done) = mpsc::sync_channel(1);

        thread::spawn(move || {
            let (mut conn, _) = listener.accept().unwrap();
            let ready = fstrm::read_control_frame(&mut conn).unwrap();
            assert_eq!(ready.frame_type, fstrm::CONTROL_READY);
            fstrm::write_control_frame(&mut conn, fstrm::CONTROL_ACCEPT, Some(CONTENT_TYPE))
                .unwrap();
            let start = fstrm::read_control_frame(&mut conn).unwrap();
            assert_eq!(start.frame_type, fstrm::CONTROL_START);
            let payload = fstrm::read_data_frame(&mut conn).unwrap();
            tx.send(payload).unwrap();
        });

        let dest = Destination::Unix(path.clone());
        let mut writer = connect_one(&dest).unwrap();
        let event = ExportEvent {
            kind: EventKind::Query,
            txn_id: 1,
            client_addr: SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 1234),
            protocol_udp: true,
            wire: vec![0xab, 0xcd],
            attempt_count: 1,
            extra: None,
        };
        let bytes = encode_event(b"test-export", &event).unwrap();
        writer.write_frame(&bytes).unwrap();
        let got = rx_done.recv_timeout(Duration::from_secs(2)).unwrap();
        assert_eq!(got, bytes);
    }

    #[cfg(unix)]
    #[test]
    fn reconnects_when_collector_appears_later() {
        use crossbeam_channel::bounded;
        use std::os::unix::net::UnixListener;

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("late.sock");
        let missing = path.clone();

        let dest = format!("unix:{}", missing.display());
        let instance = {
            let mut inst = dnstap_sink("reconnect-test", vec![dest.as_str()], None);
            inst.connect_retry = ConnectRetryConfig {
                initial_ms: 50,
                max_ms: 200,
                multiplier: 2.0,
                max_elapsed_ms: 0,
                jitter: false,
            };
            inst
        };
        let metrics = instance.metrics.clone();
        let (tx, rx) = bounded(4);
        let worker = thread::spawn(move || DnstapSink::new(instance).run(rx));

        thread::sleep(Duration::from_millis(150));
        assert!(metrics.snapshot().connect_attempts >= 1);

        let listener = UnixListener::bind(&path).unwrap();
        let acceptor = thread::spawn(move || {
            let (mut conn, _) = listener.accept().unwrap();
            let ready = fstrm::read_control_frame(&mut conn).unwrap();
            assert_eq!(ready.frame_type, fstrm::CONTROL_READY);
            fstrm::write_control_frame(&mut conn, fstrm::CONTROL_ACCEPT, Some(CONTENT_TYPE))
                .unwrap();
            let start = fstrm::read_control_frame(&mut conn).unwrap();
            assert_eq!(start.frame_type, fstrm::CONTROL_START);
            let _ = fstrm::read_data_frame(&mut conn).unwrap();
        });

        thread::sleep(Duration::from_millis(500));
        assert_eq!(metrics.snapshot().connected, 1);

        let event = ExportEvent {
            kind: EventKind::Query,
            txn_id: 1,
            client_addr: SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 1234),
            protocol_udp: true,
            wire: vec![0x01, 0x02],
            attempt_count: 1,
            extra: None,
        };
        tx.send(event).unwrap();
        thread::sleep(Duration::from_millis(300));
        acceptor.join().unwrap();
        assert!(
            metrics.snapshot().delivered >= 1,
            "expected delivered after collector appeared"
        );
        drop(tx);
        worker.join().unwrap();
    }

    #[test]
    fn parse_custom_connect_retry_from_proto() {
        let sink = EventSinkConfig {
            r#type: "dnstap".into(),
            export_id: "x".into(),
            destinations: vec!["unix:/tmp/x".into()],
            emit: vec!["query".into()],
            filters: None,
            extra_fields: vec![],
            extra_tags: vec![],
            name: None,
            connect_retry: Some(ConnectRetry {
                initial_ms: 250,
                max_ms: 8000,
                multiplier: 2.0,
                max_elapsed_ms: 0,
                jitter: false,
            }),
        };
        let cfg = parse_connect_retry(&sink).unwrap();
        assert_eq!(cfg.initial_ms, 250);
        assert_eq!(cfg.max_ms, 8000);
        assert!(!cfg.jitter);
    }
}
