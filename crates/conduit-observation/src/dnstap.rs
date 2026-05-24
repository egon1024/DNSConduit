//! Dnstap protobuf + framestream writer for one sink instance.

use crate::compile::{CompiledSinkInstance, Destination};
use crate::event::{EventKind, ObservationEvent};
use crate::fstrm;
use crate::sink::ObservationSink;
use crossbeam_channel::Receiver;
use dnstap::{ClientQuery, ClientResponse, DNSMessage, SocketFamily, SocketProtocol};
use protobuf::Message;
use std::io::{self, Read, Write};
use std::net::TcpStream;
use std::path::Path;
use std::time::{Duration, SystemTime};

const CONTENT_TYPE: &str = "protobuf:dnstap.Dnstap";
const RECONNECT_DELAY: Duration = Duration::from_secs(1);

pub struct DnstapSink {
    instance: CompiledSinkInstance,
}

impl DnstapSink {
    pub fn new(instance: CompiledSinkInstance) -> Self {
        Self { instance }
    }
}

impl ObservationSink for DnstapSink {
    fn run(self, rx: Receiver<ObservationEvent>) {
        let identity = self.instance.export_id.as_bytes().to_vec();
        let mut writers = Vec::new();
        loop {
            if writers.is_empty() {
                writers = connect_all(&self.instance.destinations);
                if writers.is_empty() {
                    std::thread::sleep(RECONNECT_DELAY);
                    continue;
                }
            }
            match rx.recv() {
                Ok(event) => {
                    let bytes = match encode_event(&identity, &event) {
                        Ok(b) => b,
                        Err(e) => {
                            tracing::warn!(error = %e, "dnstap encode failed");
                            continue;
                        }
                    };
                    writers.retain_mut(|w| w.write_frame(&bytes).is_ok());
                    if writers.is_empty() {
                        tracing::warn!("dnstap destinations disconnected, reconnecting");
                    }
                }
                Err(_) => break,
            }
        }
        for w in writers {
            let _ = w.finish();
        }
    }
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

fn encode_event(identity: &[u8], event: &ObservationEvent) -> io::Result<Vec<u8>> {
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
    use crate::event::{EventKind, ObservationEvent};
    use std::net::{IpAddr, Ipv4Addr, SocketAddr};

    #[test]
    fn encode_client_query_bytes() {
        let event = ObservationEvent {
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
        let event = ObservationEvent {
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
        use std::thread;
        use std::time::Duration;

        if Command::new("fstrm_capture")
            .arg("-h")
            .output()
            .is_err()
        {
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
        let event = ObservationEvent {
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
        // fstrm_capture buffers connection data in stdio until SIGHUP or graceful exit.
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
        use std::thread;
        use std::time::Duration;

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sink.sock");
        let listener = UnixListener::bind(&path).unwrap();
        let (tx, rx) = mpsc::sync_channel(1);

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
        let event = ObservationEvent {
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
        let got = rx.recv_timeout(Duration::from_secs(2)).unwrap();
        assert_eq!(got, bytes);
    }
}
