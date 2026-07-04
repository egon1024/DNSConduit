//! split_io runtime integration tests.

use conduit_config::{load_yaml, validate};
use conduit_core::snapshot::{RuntimeSnapshot, SnapshotStore};
use conduit_dataplane::runtime::start;
use conduit_metrics::{MetricsHub, TracingHub};
use hickory_proto::op::{Message, Query};
use hickory_proto::rr::{Name, RecordType};
use hickory_proto::serialize::binary::{BinEncodable, BinEncoder};
use std::net::UdpSocket;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

fn sample_query(id: u16) -> Vec<u8> {
    let name = Name::from_utf8("test.example.").unwrap();
    let query = Query::query(name, RecordType::A);
    let mut msg = Message::new();
    msg.set_id(id);
    msg.add_query(query);
    let mut buf = Vec::new();
    let mut encoder = BinEncoder::new(&mut buf);
    msg.emit(&mut encoder).unwrap();
    buf
}

/// Binds the upstream socket synchronously and returns its port plus the
/// responder thread handle. Binding before the caller starts the dataplane
/// guarantees the kernel buffers the forwarded query even if the responder
/// thread has not reached `recv_from` yet; otherwise the first query can be
/// dropped and the forward parks for the full timeout.
fn mock_upstream(delay: Duration) -> (u16, thread::JoinHandle<()>) {
    let sock = UdpSocket::bind("127.0.0.1:0").unwrap();
    sock.set_read_timeout(Some(Duration::from_secs(30)))
        .unwrap();
    let port = sock.local_addr().unwrap().port();
    let handle = thread::spawn(move || {
        let mut buf = [0u8; 512];
        loop {
            let Ok((len, peer)) = sock.recv_from(&mut buf) else {
                continue;
            };
            if delay > Duration::ZERO {
                thread::sleep(delay);
            }
            let mut resp = buf[..len].to_vec();
            if resp.len() >= 3 {
                resp[2] = 0x81;
                resp[3] = 0x80;
            }
            let _ = sock.send_to(&resp, peer);
        }
    });
    (port, handle)
}

/// Upstream that accepts queries but never replies, so forwards park and time
/// out. Bound synchronously (see `mock_upstream`) and returns its port.
fn mock_blackhole() -> (u16, thread::JoinHandle<()>) {
    let sock = UdpSocket::bind("127.0.0.1:0").unwrap();
    sock.set_read_timeout(Some(Duration::from_secs(30)))
        .unwrap();
    let port = sock.local_addr().unwrap().port();
    let handle = thread::spawn(move || {
        let mut buf = [0u8; 512];
        loop {
            let _ = sock.recv_from(&mut buf);
        }
    });
    (port, handle)
}

/// split_io config with metrics enabled (full) and a named backend, used to
/// assert forward attempt/duration metrics resolve the configured backend name.
fn split_io_named_metrics_config(listen_port: u16, backend_port: u16, timeout_ms: u32) -> String {
    format!(
        r#"
schema_version: 1
dataplane:
  runtime: split_io
  policy_workers: 2
  io_workers: 1
listeners:
  threads: 1
  reuse_port: false
  listeners:
    - address: "127.0.0.1:{listen_port}"
      protocol: udp
      name: lab-udp
forward:
  outstanding_per_backend: 32
  timeout_ms: {timeout_ms}
orchestrator:
  max_attempts: 1
  max_txn_duration_ms: 10000
  txn_table_capacity: 64
events:
  queue_depth: 256
  drop_policy: drop_oldest
  sinks: []
metrics:
  enabled: true
  profile: full
pools:
  - name: default
    backends:
      - name: resolver-east
        address: "127.0.0.1:{backend_port}"
        weight: 100
control:
  listen_address: "127.0.0.1:0"
"#
    )
}

fn split_io_config(listen_port: u16, backend_port: u16, capacity: u32) -> String {
    format!(
        r#"
schema_version: 1
dataplane:
  runtime: split_io
  policy_workers: 2
  io_workers: 1
listeners:
  threads: 1
  reuse_port: false
  listeners:
    - address: "127.0.0.1:{listen_port}"
      protocol: udp
forward:
  outstanding_per_backend: 32
  timeout_ms: 5000
orchestrator:
  max_attempts: 1
  max_txn_duration_ms: 10000
  txn_table_capacity: {capacity}
events:
  queue_depth: 256
  drop_policy: drop_oldest
  sinks: []
rhai:
  max_operations: 1000
  max_call_depth: 8
pools:
  - name: default
    backends:
      - address: "127.0.0.1:{backend_port}"
        weight: 100
control:
  listen_address: "127.0.0.1:0"
"#
    )
}

fn bind_ephemeral() -> u16 {
    let s = UdpSocket::bind("127.0.0.1:0").unwrap();
    s.local_addr().unwrap().port()
}

#[test]
fn split_io_concurrent_queries_with_slow_upstream() {
    let listen_port = bind_ephemeral();
    let (backend_port, _upstream) = mock_upstream(Duration::from_millis(200));

    let yaml = split_io_config(listen_port, backend_port, 64);
    let cfg = load_yaml(&yaml).unwrap();
    assert!(validate(&cfg).ok);
    let store = Arc::new(SnapshotStore::new(RuntimeSnapshot::from_config(
        cfg.clone(),
    )));
    let metrics = Arc::new(MetricsHub::from_config(&cfg));
    let tracing = Arc::new(TracingHub::from_config(&cfg));
    let handle = start(store, metrics, tracing).expect("split_io start");

    let client = UdpSocket::bind("127.0.0.1:0").unwrap();
    client
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    let target: std::net::SocketAddr = format!("127.0.0.1:{listen_port}").parse().unwrap();

    let q1 = sample_query(1);
    let q2 = sample_query(2);
    client.send_to(&q1, target).unwrap();
    let t0 = Instant::now();
    client.send_to(&q2, target).unwrap();

    let mut buf = [0u8; 512];
    let (_, _) = client.recv_from(&mut buf).expect("first response");
    let first_elapsed = t0.elapsed();
    let (_, _) = client.recv_from(&mut buf).expect("second response");

    assert!(
        first_elapsed < Duration::from_millis(400),
        "second query should not wait for first upstream delay; took {first_elapsed:?}"
    );
    handle.shutdown();
}

#[test]
fn split_io_slot_exhaustion_increments_counter() {
    let listen_port = bind_ephemeral();
    let (backend_port, _upstream) = mock_upstream(Duration::from_millis(500));

    let yaml = split_io_config(listen_port, backend_port, 2);
    let cfg = load_yaml(&yaml).unwrap();
    let store = Arc::new(SnapshotStore::new(RuntimeSnapshot::from_config(
        cfg.clone(),
    )));
    let metrics = Arc::new(MetricsHub::from_config(&cfg));
    let tracing = Arc::new(TracingHub::from_config(&cfg));
    let handle = start(store, metrics, tracing).expect("split_io start");

    let client = UdpSocket::bind("127.0.0.1:0").unwrap();
    let target: std::net::SocketAddr = format!("127.0.0.1:{listen_port}").parse().unwrap();
    for id in 1..=5u16 {
        let _ = client.send_to(&sample_query(id), target);
    }
    thread::sleep(Duration::from_millis(100));
    assert!(
        handle.txn_store.exhaustion_total() > 0,
        "expected slot pool exhaustion under cap=2 load"
    );
    handle.shutdown();
}

#[test]
fn split_io_garbage_query_does_not_consume_slot() {
    let listen_port = bind_ephemeral();
    let (backend_port, _upstream) = mock_upstream(Duration::ZERO);

    let yaml = split_io_config(listen_port, backend_port, 4);
    let cfg = load_yaml(&yaml).unwrap();
    let store = Arc::new(SnapshotStore::new(RuntimeSnapshot::from_config(
        cfg.clone(),
    )));
    let metrics = Arc::new(MetricsHub::from_config(&cfg));
    let tracing = Arc::new(TracingHub::from_config(&cfg));
    let handle = start(store, metrics, tracing).expect("split_io start");

    let client = UdpSocket::bind("127.0.0.1:0").unwrap();
    let target: std::net::SocketAddr = format!("127.0.0.1:{listen_port}").parse().unwrap();
    let _ = client.send_to(&[0xff, 0x00, 0x01], target);
    thread::sleep(Duration::from_millis(50));

    assert_eq!(handle.txn_store.in_use(), 0);
    assert_eq!(handle.txn_store.exhaustion_total(), 0);
    handle.shutdown();
}

#[test]
fn split_io_survives_idle_then_second_query() {
    let listen_port = bind_ephemeral();
    let (backend_port, _upstream) = mock_upstream(Duration::ZERO);

    let yaml = split_io_config(listen_port, backend_port, 4);
    let cfg = load_yaml(&yaml).unwrap();
    let store = Arc::new(SnapshotStore::new(RuntimeSnapshot::from_config(
        cfg.clone(),
    )));
    let metrics = Arc::new(MetricsHub::from_config(&cfg));
    let tracing = Arc::new(TracingHub::from_config(&cfg));
    let handle = start(store, metrics, tracing).expect("split_io start");

    let client = UdpSocket::bind("127.0.0.1:0").unwrap();
    client
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    let target: std::net::SocketAddr = format!("127.0.0.1:{listen_port}").parse().unwrap();

    client.send_to(&sample_query(11), target).unwrap();
    let mut buf = [0u8; 512];
    let (_, _) = client.recv_from(&mut buf).expect("first response");

    thread::sleep(Duration::from_secs(3));

    client.send_to(&sample_query(12), target).unwrap();
    let (_, _) = client
        .recv_from(&mut buf)
        .expect("second response after idle");

    handle.shutdown();
}

#[test]
fn split_io_records_forward_success_metric_with_backend_name() {
    let listen_port = bind_ephemeral();
    let (backend_port, _upstream) = mock_upstream(Duration::ZERO);

    let yaml = split_io_named_metrics_config(listen_port, backend_port, 5000);
    let cfg = load_yaml(&yaml).unwrap();
    assert!(validate(&cfg).ok);
    let store = Arc::new(SnapshotStore::new(RuntimeSnapshot::from_config(
        cfg.clone(),
    )));
    let metrics = Arc::new(MetricsHub::from_config(&cfg));
    let tracing = Arc::new(TracingHub::from_config(&cfg));
    let handle = start(store, metrics.clone(), tracing).expect("split_io start");

    let client = UdpSocket::bind("127.0.0.1:0").unwrap();
    client
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    let target: std::net::SocketAddr = format!("127.0.0.1:{listen_port}").parse().unwrap();
    client.send_to(&sample_query(21), target).unwrap();
    let mut buf = [0u8; 512];
    let (_, _) = client.recv_from(&mut buf).expect("response from split_io");

    let body = conduit_metrics::render_prometheus(&metrics, &[]);
    assert!(
        body.contains(
            r#"conduit_forward_attempts_total{backend="resolver-east",outcome="success",pool="default"} 1"#
        ),
        "expected named-backend success attempt; body:\n{body}"
    );
    assert!(
        body.contains(
            r#"conduit_forward_duration_seconds_count{backend="resolver-east",pool="default"} 1"#
        ),
        "expected forward duration observation for named backend; body:\n{body}"
    );
    assert!(
        body.contains(r#"listener="lab-udp""#),
        "expected listener label to use the configured name; body:\n{body}"
    );
    assert!(
        !body.contains(&format!(r#"listener="127.0.0.1:{listen_port}""#)),
        "listener label must be the name, not the bind address; body:\n{body}"
    );
    handle.shutdown();
}

#[test]
fn split_io_records_forward_timeout_metric_with_pool_and_name() {
    let listen_port = bind_ephemeral();
    let (dead_port, _blackhole) = mock_blackhole();

    let yaml = split_io_named_metrics_config(listen_port, dead_port, 300);
    let cfg = load_yaml(&yaml).unwrap();
    let store = Arc::new(SnapshotStore::new(RuntimeSnapshot::from_config(
        cfg.clone(),
    )));
    let metrics = Arc::new(MetricsHub::from_config(&cfg));
    let tracing = Arc::new(TracingHub::from_config(&cfg));
    let handle = start(store, metrics.clone(), tracing).expect("split_io start");

    let client = UdpSocket::bind("127.0.0.1:0").unwrap();
    client
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    let target: std::net::SocketAddr = format!("127.0.0.1:{listen_port}").parse().unwrap();
    client.send_to(&sample_query(31), target).unwrap();
    // SERVFAIL is returned after the forward times out (~300 ms); the metric is
    // recorded on the WaitResponse resume before Send, so it exists by then.
    let mut buf = [0u8; 512];
    let _ = client.recv_from(&mut buf);
    thread::sleep(Duration::from_millis(50));

    let body = conduit_metrics::render_prometheus(&metrics, &[]);
    assert!(
        body.contains(
            r#"conduit_forward_attempts_total{backend="resolver-east",outcome="error",pool="default"} 1"#
        ),
        "expected named-backend timeout attempt with real pool; body:\n{body}"
    );
    assert!(
        body.contains(
            r#"conduit_forward_errors_total{backend="resolver-east",pool="default",reason="timeout"} 1"#
        ),
        "expected timeout forward error on the named backend; body:\n{body}"
    );
    assert!(
        !body.contains(r#"pool="unknown""#),
        "timeout must not record pool=\"unknown\"; body:\n{body}"
    );
    handle.shutdown();
}

#[test]
fn split_io_valid_query_reaches_upstream() {
    let listen_port = bind_ephemeral();
    let (backend_port, _upstream) = mock_upstream(Duration::ZERO);

    let yaml = split_io_config(listen_port, backend_port, 4);
    let cfg = load_yaml(&yaml).unwrap();
    let store = Arc::new(SnapshotStore::new(RuntimeSnapshot::from_config(
        cfg.clone(),
    )));
    let metrics = Arc::new(MetricsHub::from_config(&cfg));
    let tracing = Arc::new(TracingHub::from_config(&cfg));
    let handle = start(store, metrics, tracing).expect("split_io start");

    let client = UdpSocket::bind("127.0.0.1:0").unwrap();
    client
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    let target: std::net::SocketAddr = format!("127.0.0.1:{listen_port}").parse().unwrap();
    client.send_to(&sample_query(7), target).unwrap();
    let mut buf = [0u8; 512];
    let (_, _) = client.recv_from(&mut buf).expect("response from split_io");
    handle.shutdown();
}
