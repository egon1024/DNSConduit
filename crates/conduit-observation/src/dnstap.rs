//! Dnstap protobuf + framestream writer for one sink instance.

use crate::compile::{CompiledSinkInstance, Destination};
use crate::event::{EventKind, ObservationEvent};
use crate::sink::ObservationSink;
use crossbeam_channel::Receiver;
use dnstap::{ClientQuery, ClientResponse, DNSMessage, SocketFamily, SocketProtocol};
use framestream::EncoderWriter;
use protobuf::Message;
use std::io::{self, BufWriter, Write};
use std::net::{IpAddr, SocketAddr, TcpStream};
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

struct FrameWriter {
    encoder: EncoderWriter<BufWriter<Box<dyn Write + Send>>>,
}

impl FrameWriter {
    fn write_frame(&mut self, frame: &[u8]) -> io::Result<()> {
        self.encoder.write_all(frame)?;
        self.encoder.flush()
    }

    fn finish(self) -> io::Result<()> {
        self.encoder.finish().map(|_| ())
    }
}

fn connect_all(destinations: &[Destination]) -> Vec<FrameWriter> {
    destinations
        .iter()
        .filter_map(|d| connect_one(d).ok())
        .collect()
}

fn connect_one(dest: &Destination) -> io::Result<FrameWriter> {
    let stream: Box<dyn Write + Send> = match dest {
        Destination::Unix(path) => Box::new(unix_connect(path)?),
        Destination::Tcp { host, port } => {
            let addr = format!("{host}:{port}");
            let stream = TcpStream::connect(addr)?;
            stream.set_nodelay(true)?;
            Box::new(stream)
        }
    };
    let encoder = EncoderWriter::new(BufWriter::new(stream), Some(CONTENT_TYPE.to_string()));
    Ok(FrameWriter { encoder })
}

#[cfg(unix)]
fn unix_connect(path: &Path) -> io::Result<std::os::unix::net::UnixStream> {
    use std::os::unix::net::UnixStream;
    UnixStream::connect(path)
}

#[cfg(not(unix))]
fn unix_connect(_path: &Path) -> io::Result<Box<dyn Write + Send>> {
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
            dns.response_address = Some(event.client_addr.ip());
            dns.response_port = Some(event.client_addr.port());
        }
    }
    let mut pb = dns.into_protobuf();
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

fn socket_meta(addr: SocketAddr, udp: bool) -> (SocketFamily, SocketProtocol) {
    let family = match addr.ip() {
        IpAddr::V4(_) => SocketFamily::INET,
        IpAddr::V6(_) => SocketFamily::INET6,
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
        };
        let bytes = encode_event(b"conduit", &event).unwrap();
        assert!(!bytes.is_empty());
    }
}
