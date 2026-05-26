//! TCP upstream forward (RFC 7766).

use conduit_config::forward::{CompiledForward, UpstreamTransport};
use conduit_dataplane::forward::tcp::forward_tcp;
use hickory_proto::op::{Message, Query, ResponseCode};
use hickory_proto::rr::{Name, RecordType};
use hickory_proto::serialize::binary::{BinEncodable, BinEncoder};
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

fn sample_query() -> Vec<u8> {
    let name = Name::from_utf8("test.example.").unwrap();
    let query = Query::query(name, RecordType::A);
    let mut msg = Message::new();
    let mut header = *msg.header();
    header.set_id(0x1234);
    msg.set_header(header);
    msg.add_query(query);
    let mut out = Vec::new();
    let mut enc = BinEncoder::new(&mut out);
    msg.emit(&mut enc).unwrap();
    out
}

fn sample_response() -> Vec<u8> {
    let mut msg = Message::new();
    let mut header = *msg.header();
    header.set_id(0x1234);
    msg.set_header(header);
    msg.set_response_code(ResponseCode::NoError);
    let mut out = Vec::new();
    let mut enc = BinEncoder::new(&mut out);
    msg.emit(&mut enc).unwrap();
    out
}

#[test]
fn tcp_forward_rfc7766_round_trip() {
    let (port_tx, port_rx) = mpsc::channel();
    let server = thread::spawn(move || {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        port_tx.send(listener.local_addr().unwrap().port()).unwrap();
        let (mut stream, _) = listener.accept().unwrap();
        let mut len_buf = [0u8; 2];
        stream.read_exact(&mut len_buf).unwrap();
        let n = usize::from(u16::from_be_bytes(len_buf));
        let mut q = vec![0u8; n];
        stream.read_exact(&mut q).unwrap();
        assert!(!q.is_empty());
        let resp = sample_response();
        let len = (resp.len() as u16).to_be_bytes();
        stream.write_all(&len).unwrap();
        stream.write_all(&resp).unwrap();
    });

    let port = port_rx.recv().unwrap();
    let backend: SocketAddr = format!("127.0.0.1:{port}").parse().unwrap();
    let q = sample_query();
    let wire =
        forward_tcp(backend, &q, Duration::from_millis(2000), None, None).expect("tcp forward");
    let msg = Message::from_vec(&wire).unwrap();
    assert_eq!(msg.id(), 0x1234);
    server.join().unwrap();
}

#[test]
fn compiled_forward_defaults_udp_only() {
    let f = CompiledForward {
        sources_v4: vec![],
        sources_v6: vec![],
        source_selection: "round_robin".into(),
        upstream_transport: UpstreamTransport::default(),
        client_tcp_uses_upstream_tcp: false,
        timeout_ms: 1000,
        outstanding_per_backend: 10,
    };
    assert_eq!(f.upstream_transport, UpstreamTransport::UdpOnly);
}
