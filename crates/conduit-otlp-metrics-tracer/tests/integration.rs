//! Tracer accepts OTLP-shaped POSTs and exposes accept counts on /stats.

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener as StdTcpListener, TcpStream};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

fn free_port() -> u16 {
    let listener = StdTcpListener::bind("127.0.0.1:0").expect("bind ephemeral");
    listener.local_addr().expect("local_addr").port()
}

fn wait_ready(addr: SocketAddr, timeout: Duration) {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if TcpStream::connect_timeout(&addr, Duration::from_millis(50)).is_ok() {
            return;
        }
        thread::sleep(Duration::from_millis(25));
    }
    panic!("tracer not ready at {addr}");
}

fn http_exchange(addr: SocketAddr, request_head: &str, body: &[u8]) -> String {
    let mut stream = TcpStream::connect(addr).expect("connect");
    stream
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    stream
        .write_all(request_head.as_bytes())
        .expect("write head");
    if !body.is_empty() {
        stream.write_all(body).expect("write body");
    }
    let mut buf = Vec::new();
    let _ = stream.read_to_end(&mut buf);
    String::from_utf8_lossy(&buf).into_owned()
}

#[test]
fn tracer_accepts_post_and_reports_stats() {
    let port = free_port();
    let addr: SocketAddr = format!("127.0.0.1:{port}").parse().unwrap();
    let tracer_bin = env!("CARGO_BIN_EXE_conduit-otlp-metrics-tracer");

    let mut child = Command::new(tracer_bin)
        .args(["-a", &addr.to_string(), "-p", "/v1/metrics", "-f", "log"])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn tracer");

    wait_ready(addr, Duration::from_secs(5));

    let body = b"fake-otlp-protobuf-body";
    let post_head = format!(
        "POST /v1/metrics HTTP/1.1\r\nHost: {addr}\r\nContent-Type: application/x-protobuf\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    let post_resp = http_exchange(addr, &post_head, body);
    assert!(
        post_resp.contains("200"),
        "expected HTTP 200, got: {post_resp}"
    );

    let stats = http_exchange(
        addr,
        &format!("GET /stats HTTP/1.1\r\nHost: {addr}\r\nConnection: close\r\n\r\n"),
        &[],
    );
    assert!(
        stats.contains("\"accepts\":1"),
        "expected accepts=1 in /stats, got: {stats}"
    );
    assert!(
        stats.contains("\"failures\":0"),
        "expected failures=0 in /stats, got: {stats}"
    );

    let _ = child.kill();
    let output = child.wait_with_output().expect("wait");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("accepts=1"),
        "expected accept debug line on stdout: {stdout}"
    );
}
