//! split_io runtime integration tests.

use conduit_config::{load_yaml, validate};
use conduit_core::snapshot::{RuntimeSnapshot, SnapshotStore};
use conduit_dataplane::runtime::{start, DataplaneHandle};
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

fn split_io_config_workers(
    listen_port: u16,
    backend_port: u16,
    capacity: u32,
    policy_workers: u32,
    io_workers: u32,
) -> String {
    split_io_config_ingress_workers(
        listen_port,
        backend_port,
        capacity,
        1,
        policy_workers,
        io_workers,
    )
}

fn split_io_config_ingress_workers(
    listen_port: u16,
    backend_port: u16,
    capacity: u32,
    ingress_threads: u32,
    policy_workers: u32,
    io_workers: u32,
) -> String {
    let reuse_port = ingress_threads > 1;
    format!(
        r#"
schema_version: 1
dataplane:
  runtime: split_io
  policy_workers: {policy_workers}
  io_workers: {io_workers}
listeners:
  threads: {ingress_threads}
  reuse_port: {reuse_port}
  listeners:
    - address: "127.0.0.1:{listen_port}"
      protocol: udp
forward:
  outstanding_per_backend: 64
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

fn split_io_config(listen_port: u16, backend_port: u16, capacity: u32) -> String {
    split_io_config_workers(listen_port, backend_port, capacity, 2, 1)
}

fn reserve_listen_port() -> u16 {
    let sock = UdpSocket::bind("127.0.0.1:0").unwrap();
    sock.local_addr().unwrap().port()
}

/// Bind split_io on an ephemeral listen port, retrying when parallel tests win the race.
fn start_split_io<F>(mut build_yaml: F, label: &str) -> (DataplaneHandle, u16)
where
    F: FnMut(u16) -> String,
{
    use std::io::ErrorKind;
    for _ in 0..64 {
        let listen_port = reserve_listen_port();
        let yaml = build_yaml(listen_port);
        let cfg = load_yaml(&yaml).unwrap();
        let store = Arc::new(SnapshotStore::new(RuntimeSnapshot::from_config(
            cfg.clone(),
        )));
        let metrics = Arc::new(MetricsHub::from_config(&cfg));
        let tracing = Arc::new(TracingHub::from_config(&cfg));
        match start(store, metrics, tracing) {
            Ok(handle) => return (handle, listen_port),
            Err(e) if e.kind() == ErrorKind::AddrInUse => continue,
            Err(e) => panic!("{label}: {e}"),
        }
    }
    panic!("{label}: no free listen port after retries");
}

fn start_split_io_validated<F>(mut build_yaml: F, label: &str) -> (DataplaneHandle, u16)
where
    F: FnMut(u16) -> String,
{
    use std::io::ErrorKind;
    for _ in 0..64 {
        let listen_port = reserve_listen_port();
        let yaml = build_yaml(listen_port);
        let cfg = load_yaml(&yaml).unwrap();
        assert!(validate(&cfg).ok, "{label}: invalid config");
        let store = Arc::new(SnapshotStore::new(RuntimeSnapshot::from_config(
            cfg.clone(),
        )));
        let metrics = Arc::new(MetricsHub::from_config(&cfg));
        let tracing = Arc::new(TracingHub::from_config(&cfg));
        match start(store, metrics, tracing) {
            Ok(handle) => return (handle, listen_port),
            Err(e) if e.kind() == ErrorKind::AddrInUse => continue,
            Err(e) => panic!("{label}: {e}"),
        }
    }
    panic!("{label}: no free listen port after retries");
}

fn start_split_io_with_metrics<F>(
    mut build_yaml: F,
    label: &str,
) -> (DataplaneHandle, u16, Arc<MetricsHub>)
where
    F: FnMut(u16) -> String,
{
    use std::io::ErrorKind;
    for _ in 0..64 {
        let listen_port = reserve_listen_port();
        let yaml = build_yaml(listen_port);
        let cfg = load_yaml(&yaml).unwrap();
        assert!(validate(&cfg).ok, "{label}: invalid config");
        let store = Arc::new(SnapshotStore::new(RuntimeSnapshot::from_config(
            cfg.clone(),
        )));
        let metrics = Arc::new(MetricsHub::from_config(&cfg));
        let tracing = Arc::new(TracingHub::from_config(&cfg));
        match start(store, metrics.clone(), tracing) {
            Ok(handle) => return (handle, listen_port, metrics),
            Err(e) if e.kind() == ErrorKind::AddrInUse => continue,
            Err(e) => panic!("{label}: {e}"),
        }
    }
    panic!("{label}: no free listen port after retries");
}

#[test]
fn split_io_concurrent_queries_with_slow_upstream() {
    let (backend_port, _upstream) = mock_upstream(Duration::from_millis(200));
    let (handle, listen_port) = start_split_io(
        |listen_port| split_io_config(listen_port, backend_port, 64),
        "split_io start",
    );

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
    let (backend_port, _upstream) = mock_upstream(Duration::from_millis(500));
    let (handle, listen_port) = start_split_io(
        |listen_port| split_io_config(listen_port, backend_port, 2),
        "split_io start",
    );

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
    let (backend_port, _upstream) = mock_upstream(Duration::ZERO);
    let (handle, listen_port) = start_split_io(
        |listen_port| split_io_config(listen_port, backend_port, 4),
        "split_io start",
    );

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
    let (backend_port, _upstream) = mock_upstream(Duration::ZERO);
    let (handle, listen_port) = start_split_io(
        |listen_port| split_io_config(listen_port, backend_port, 4),
        "split_io start",
    );

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
    let (backend_port, _upstream) = mock_upstream(Duration::ZERO);
    let (handle, listen_port, metrics) = start_split_io_with_metrics(
        |listen_port| split_io_named_metrics_config(listen_port, backend_port, 5000),
        "split_io start",
    );

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
    let (dead_port, _blackhole) = mock_blackhole();
    let (handle, listen_port, metrics) = start_split_io_with_metrics(
        |listen_port| split_io_named_metrics_config(listen_port, dead_port, 300),
        "split_io start",
    );

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
    assert!(
        body.contains(
            r#"conduit_lookup_no_answer_total{pool="default",profile="default",reason="forward_error"} 1"#
        ),
        "expected NoAnswer convergence metric after forward timeout; body:\n{body}"
    );
    handle.shutdown();
}

#[test]
fn split_io_valid_query_reaches_upstream() {
    let (backend_port, _upstream) = mock_upstream(Duration::ZERO);
    let (handle, listen_port) = start_split_io(
        |listen_port| split_io_config(listen_port, backend_port, 4),
        "split_io start",
    );

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

/// Multi I/O worker + multi policy worker under a fast stub: every query must
/// resume exactly once (no lost replies across shards).
#[test]
fn split_io_multi_io_workers_concurrent_fast_upstream() {
    let (backend_port, _upstream) = mock_upstream(Duration::ZERO);
    let (handle, listen_port) = start_split_io_validated(
        |listen_port| split_io_config_workers(listen_port, backend_port, 128, 4, 4),
        "split_io multi-io start",
    );

    let client = UdpSocket::bind("127.0.0.1:0").unwrap();
    client
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    let target: std::net::SocketAddr = format!("127.0.0.1:{listen_port}").parse().unwrap();

    const N: u16 = 64;
    for id in 1..=N {
        client.send_to(&sample_query(id), target).unwrap();
    }

    let mut buf = [0u8; 512];
    let mut got = 0u16;
    for _ in 1..=N {
        match client.recv_from(&mut buf) {
            Ok(_) => got += 1,
            Err(e) => {
                panic!(
                    "every concurrent submit must resume under multi io_workers: {e} (got {got}/{N})"
                );
            }
        }
    }
    handle.shutdown();
}

/// Multi policy workers + fast stub: every query must resume (no
/// "slot was not in IoWait" drop storm / parked-forever slots).
#[test]
fn split_io_multi_policy_fast_upstream_no_resume_drops() {
    let (backend_port, _upstream) = mock_upstream(Duration::ZERO);
    let (handle, listen_port) = start_split_io_validated(
        |listen_port| split_io_config_workers(listen_port, backend_port, 256, 4, 2),
        "split_io multi-policy start",
    );

    let client = UdpSocket::bind("127.0.0.1:0").unwrap();
    client
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    let target: std::net::SocketAddr = format!("127.0.0.1:{listen_port}").parse().unwrap();

    const N: u16 = 128;
    for id in 1..=N {
        client.send_to(&sample_query(id), target).unwrap();
    }

    let mut buf = [0u8; 512];
    let mut got = 0u16;
    for _ in 1..=N {
        match client.recv_from(&mut buf) {
            Ok(_) => got += 1,
            Err(e) => {
                panic!("multi-policy fast upstream must not drop resumes: {e} (got {got}/{N})");
            }
        }
    }
    assert_eq!(
        got, N,
        "multi-policy fast upstream must deliver every reply (got {got}/{N})"
    );
    // Reply delivery precedes release_slot; wait briefly so slots return to Free.
    let deadline = Instant::now() + Duration::from_secs(2);
    while handle.txn_store.in_use() > 0 && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(
        handle.txn_store.in_use(),
        0,
        "no slots parked forever after replies"
    );
    handle.shutdown();
}

/// Multi ingress + multi policy under a fast stub: every UDP reply must land
/// (sharded ReplyRoutes + PolicyQueue must not lose routes or New/Resume pairs).
#[test]
fn split_io_multi_ingress_multi_policy_no_lost_replies() {
    let (backend_port, _upstream) = mock_upstream(Duration::ZERO);
    let (handle, listen_port) = start_split_io_validated(
        |listen_port| split_io_config_ingress_workers(listen_port, backend_port, 256, 4, 4, 2),
        "split_io multi-ingress start",
    );

    let client = UdpSocket::bind("127.0.0.1:0").unwrap();
    client
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    let target: std::net::SocketAddr = format!("127.0.0.1:{listen_port}").parse().unwrap();

    const N: u16 = 128;
    for id in 1..=N {
        client.send_to(&sample_query(id), target).unwrap();
    }

    let mut buf = [0u8; 512];
    let mut got = 0u16;
    for _ in 1..=N {
        match client.recv_from(&mut buf) {
            Ok(_) => got += 1,
            Err(e) => {
                panic!(
                    "multi-ingress + multi-policy must not lose UDP replies: {e} (got {got}/{N})"
                );
            }
        }
    }
    assert_eq!(got, N);
    let deadline = Instant::now() + Duration::from_secs(2);
    while handle.txn_store.in_use() > 0 && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(
        handle.txn_store.in_use(),
        0,
        "New then Resume must complete; no parked slots"
    );
    handle.shutdown();
}

/// io_workers: 1 regression — same concurrent slow-upstream property as the
/// historic single-poller path.
#[test]
fn split_io_io_workers_one_matches_single_poller_concurrency() {
    let (backend_port, _upstream) = mock_upstream(Duration::from_millis(150));
    let (handle, listen_port) = start_split_io(
        |listen_port| split_io_config_workers(listen_port, backend_port, 64, 2, 1),
        "split_io io_workers=1 start",
    );

    let client = UdpSocket::bind("127.0.0.1:0").unwrap();
    client
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    let target: std::net::SocketAddr = format!("127.0.0.1:{listen_port}").parse().unwrap();

    let q1 = sample_query(101);
    let q2 = sample_query(102);
    client.send_to(&q1, target).unwrap();
    let t0 = Instant::now();
    client.send_to(&q2, target).unwrap();

    let mut buf = [0u8; 512];
    let (_, _) = client.recv_from(&mut buf).expect("first response");
    let first_elapsed = t0.elapsed();
    let (_, _) = client.recv_from(&mut buf).expect("second response");

    assert!(
        first_elapsed < Duration::from_millis(350),
        "io_workers=1 must still overlap slow upstream waits; took {first_elapsed:?}"
    );
    handle.shutdown();
}
