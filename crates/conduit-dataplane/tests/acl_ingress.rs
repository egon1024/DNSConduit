//! Client ACL ingress: sync and split_io parity for drop / refuse / accept (UDP + TCP).

use conduit_config::{load_yaml, validate};
use conduit_core::snapshot::{RuntimeSnapshot, SnapshotStore};
use conduit_dataplane::runtime::start;
use conduit_dataplane::runtime::DataplaneHandle;
use conduit_metrics::{encode_builtin, MetricsHub, TracingHub};
use hickory_proto::op::{Edns, Message, Query, ResponseCode};
use hickory_proto::rr::rdata::opt::{ClientSubnet, EdnsOption};
use hickory_proto::rr::{Name, RecordType};
use hickory_proto::serialize::binary::{BinDecodable, BinEncodable, BinEncoder};
use std::io::{Read, Write};
use std::net::{IpAddr, Ipv4Addr, TcpStream, UdpSocket};
use std::path::PathBuf;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

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

/// Query with EDNS Client Subnet for a prefix that is *not* the socket peer.
fn sample_query_with_ecs(id: u16, ecs_addr: IpAddr, source_prefix: u8) -> Vec<u8> {
    let name = Name::from_utf8("test.example.").unwrap();
    let query = Query::query(name, RecordType::A);
    let mut msg = Message::new();
    msg.set_id(id);
    msg.add_query(query);
    let mut edns = Edns::new();
    edns.set_max_payload(1232);
    edns.options_mut()
        .insert(EdnsOption::Subnet(ClientSubnet::new(
            ecs_addr,
            source_prefix,
            0,
        )));
    msg.set_edns(edns);
    let mut buf = Vec::new();
    let mut encoder = BinEncoder::new(&mut buf);
    msg.emit(&mut encoder).unwrap();
    buf
}

fn assert_no_txn_slot_consumed(handle: &DataplaneHandle, label: &str) {
    assert_eq!(
        handle.txn_store.in_use(),
        0,
        "{label}: terminal ACL deny must leave txn slots free"
    );
    assert_eq!(
        handle.txn_store.exhaustion_total(),
        0,
        "{label}: terminal ACL deny must not attempt slot acquire"
    );
}

fn wait_slots_in_use(handle: &DataplaneHandle, want: u32, timeout: Duration) {
    let deadline = Instant::now() + timeout;
    loop {
        if handle.txn_store.in_use() >= want {
            return;
        }
        if Instant::now() >= deadline {
            panic!(
                "timed out waiting for {want} in-use slot(s); have {}",
                handle.txn_store.in_use()
            );
        }
        thread::sleep(Duration::from_millis(5));
    }
}

fn mock_upstream() -> (u16, thread::JoinHandle<()>) {
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

fn bind_ephemeral() -> u16 {
    let s = UdpSocket::bind("127.0.0.1:0").unwrap();
    s.local_addr().unwrap().port()
}

fn write_cidr(tag: &str, contents: &str) -> PathBuf {
    let mut path = std::env::temp_dir();
    let unique = format!(
        "conduit-acl-cidr-{tag}-{}-{}.txt",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );
    path.push(unique);
    let mut f = std::fs::File::create(&path).unwrap();
    f.write_all(contents.as_bytes()).unwrap();
    path
}

fn acl_config(
    runtime: &str,
    protocol: &str,
    listen_port: u16,
    backend_port: u16,
    cidr_path: &str,
    acls_yaml: &str,
) -> String {
    format!(
        r#"
schema_version: 1
dataplane:
  runtime: {runtime}
  policy_workers: 1
  io_workers: 1
listeners:
  threads: 1
  reuse_port: false
  listeners:
    - address: "127.0.0.1:{listen_port}"
      protocol: {protocol}
      name: acl-lab
forward:
  outstanding_per_backend: 32
  timeout_ms: 2000
orchestrator:
  max_attempts: 1
  max_txn_duration_ms: 5000
  txn_table_capacity: 16
events:
  queue_depth: 64
  drop_policy: drop_oldest
  sinks: []
metrics:
  enabled: true
  profile: full
data_sources:
  - name: nets
    type: cidr
    path: {cidr_path}
{acls_yaml}
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

fn start_runtime(
    yaml: &str,
) -> (
    conduit_dataplane::runtime::DataplaneHandle,
    Arc<MetricsHub>,
    Arc<SnapshotStore>,
) {
    let cfg = load_yaml(yaml).expect("load");
    assert!(validate(&cfg).ok, "{:?}", validate(&cfg).errors);
    let store = Arc::new(SnapshotStore::new(RuntimeSnapshot::from_config(
        cfg.clone(),
    )));
    let metrics = Arc::new(MetricsHub::from_config(&cfg));
    let tracing = Arc::new(TracingHub::from_config(&cfg));
    let handle = start(store.clone(), metrics.clone(), tracing).expect("start");
    // Give listeners a moment to bind.
    thread::sleep(Duration::from_millis(50));
    (handle, metrics, store)
}

fn send_and_recv(listen_port: u16, id: u16) -> Option<Message> {
    let client = UdpSocket::bind("127.0.0.1:0").unwrap();
    client
        .set_read_timeout(Some(Duration::from_millis(400)))
        .unwrap();
    let target: std::net::SocketAddr = format!("127.0.0.1:{listen_port}").parse().unwrap();
    client.send_to(&sample_query(id), target).unwrap();
    let mut buf = [0u8; 512];
    match client.recv_from(&mut buf) {
        Ok((len, _)) => Some(Message::from_bytes(&buf[..len]).expect("parse response")),
        Err(_) => None,
    }
}

fn send_and_recv_tcp(listen_port: u16, id: u16) -> Option<Message> {
    let addr: std::net::SocketAddr = format!("127.0.0.1:{listen_port}").parse().unwrap();
    let mut stream = TcpStream::connect(addr).ok()?;
    stream
        .set_read_timeout(Some(Duration::from_millis(400)))
        .unwrap();
    stream
        .set_write_timeout(Some(Duration::from_millis(400)))
        .unwrap();
    let q = sample_query(id);
    let len = (q.len() as u16).to_be_bytes();
    stream.write_all(&len).ok()?;
    stream.write_all(&q).ok()?;
    let mut len_buf = [0u8; 2];
    stream.read_exact(&mut len_buf).ok()?;
    let n = u16::from_be_bytes(len_buf) as usize;
    if n == 0 || n > 65535 {
        return None;
    }
    let mut buf = vec![0u8; n];
    stream.read_exact(&mut buf).ok()?;
    Some(Message::from_bytes(&buf).expect("parse TCP response"))
}

fn assert_acl_drop(runtime: &str, protocol: &str) {
    let listen_port = bind_ephemeral();
    let (backend_port, _up) = mock_upstream();
    // Only 10/8 — loopback clients miss and default deny → silent drop.
    let cidr = write_cidr("drop", "10.0.0.0/8\n");
    let yaml = acl_config(
        runtime,
        protocol,
        listen_port,
        backend_port,
        cidr.to_str().unwrap(),
        r#"
acls:
  default_action: deny
  rules:
    - match: nets
      action: accept
"#,
    );
    let (handle, metrics, _) = start_runtime(&yaml);
    let reply = if protocol == "tcp" {
        send_and_recv_tcp(listen_port, 1)
    } else {
        send_and_recv(listen_port, 1)
    };
    assert!(
        reply.is_none(),
        "{runtime}/{protocol}: ACL drop must not reply"
    );
    assert_no_txn_slot_consumed(&handle, &format!("{runtime}/{protocol} drop"));
    let body = encode_builtin(metrics.builtin().gather());
    assert!(
        body.contains("conduit_acl_decisions_total") && body.contains(r#"action="drop""#),
        "{runtime}/{protocol}: expected drop metric, body:\n{body}"
    );
    handle.shutdown();
    let _ = std::fs::remove_file(cidr);
}

fn assert_acl_refuse(runtime: &str, protocol: &str) {
    let listen_port = bind_ephemeral();
    let (backend_port, _up) = mock_upstream();
    let cidr = write_cidr("refuse", "127.0.0.0/8\n");
    let yaml = acl_config(
        runtime,
        protocol,
        listen_port,
        backend_port,
        cidr.to_str().unwrap(),
        r#"
acls:
  default_action: allow
  rules:
    - match: nets
      action: refuse
"#,
    );
    let (handle, _, _) = start_runtime(&yaml);
    let msg = if protocol == "tcp" {
        send_and_recv_tcp(listen_port, 2)
    } else {
        send_and_recv(listen_port, 2)
    }
    .expect("REFUSED response");
    assert_eq!(msg.response_code(), ResponseCode::Refused);
    assert_no_txn_slot_consumed(&handle, &format!("{runtime}/{protocol} refuse"));
    handle.shutdown();
    let _ = std::fs::remove_file(cidr);
}

fn assert_acl_accept_allowlist(runtime: &str, protocol: &str) {
    let listen_port = bind_ephemeral();
    let (backend_port, _up) = mock_upstream();
    let cidr = write_cidr("accept", "127.0.0.0/8\n");
    let yaml = acl_config(
        runtime,
        protocol,
        listen_port,
        backend_port,
        cidr.to_str().unwrap(),
        r#"
acls:
  default_action: deny
  rules:
    - match: nets
      action: accept
"#,
    );
    let (handle, _, _) = start_runtime(&yaml);
    let msg = if protocol == "tcp" {
        send_and_recv_tcp(listen_port, 3)
    } else {
        send_and_recv(listen_port, 3)
    }
    .expect("forwarded response");
    assert_ne!(msg.response_code(), ResponseCode::Refused);
    handle.shutdown();
    let _ = std::fs::remove_file(cidr);
}

fn assert_acl_tag_then_rule_drop(runtime: &str, protocol: &str) {
    let listen_port = bind_ephemeral();
    let (backend_port, _up) = mock_upstream();
    let cidr = write_cidr("tag", "127.0.0.0/8\n");
    let yaml = acl_config(
        runtime,
        protocol,
        listen_port,
        backend_port,
        cidr.to_str().unwrap(),
        r#"
acls:
  default_action: allow
  rules:
    - match: nets
      action: tag
      tag: marked
rules:
  rules:
    - name: drop-marked
      hook: request
      selectors:
        - type: tag
          value: marked
      actions:
        - type: drop
"#,
    );
    let (handle, _, _) = start_runtime(&yaml);
    let reply = if protocol == "tcp" {
        send_and_recv_tcp(listen_port, 4)
    } else {
        send_and_recv(listen_port, 4)
    };
    assert!(
        reply.is_none(),
        "{runtime}/{protocol}: tagged client should be dropped by request rule"
    );
    handle.shutdown();
    let _ = std::fs::remove_file(cidr);
}

/// Explicit Tier 0 `drop` must close the TCP session before waiting to read a query.
fn assert_tcp_tier0_drop_closes_before_query_read(runtime: &str) {
    let listen_port = bind_ephemeral();
    let (backend_port, _up) = mock_upstream();
    let cidr = write_cidr("tier0", "127.0.0.0/8\n");
    let yaml = acl_config(
        runtime,
        "tcp",
        listen_port,
        backend_port,
        cidr.to_str().unwrap(),
        r#"
acls:
  default_action: allow
  rules:
    - match: nets
      action: drop
"#,
    );
    let (handle, metrics, _) = start_runtime(&yaml);
    let addr: std::net::SocketAddr = format!("127.0.0.1:{listen_port}").parse().unwrap();
    let mut stream = TcpStream::connect(addr).expect("connect");
    stream
        .set_read_timeout(Some(Duration::from_millis(500)))
        .unwrap();
    // Do not send a query. Tier 0 must close immediately; waiting for the server's
    // 5s read timeout would surface as TimedOut here.
    let mut buf = [0u8; 1];
    match stream.read(&mut buf) {
        Ok(0) => {}
        Err(e)
            if matches!(
                e.kind(),
                std::io::ErrorKind::ConnectionReset
                    | std::io::ErrorKind::ConnectionAborted
                    | std::io::ErrorKind::BrokenPipe
            ) => {}
        Err(e)
            if matches!(
                e.kind(),
                std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
            ) =>
        {
            panic!(
                "{runtime}: Tier 0 drop must close TCP before query read; got read timeout: {e}"
            );
        }
        other => panic!("{runtime}: unexpected TCP read after Tier 0 drop: {other:?}"),
    }
    let body = encode_builtin(metrics.builtin().gather());
    assert!(
        body.contains("conduit_acl_decisions_total")
            && body.contains(r#"action="drop""#)
            && body.contains(r#"tier="preadmission""#),
        "{runtime}: expected Tier 0 drop metric, body:\n{body}"
    );
    handle.shutdown();
    let _ = std::fs::remove_file(cidr);
}

#[test]
fn sync_acl_drop_no_reply() {
    assert_acl_drop("sync", "udp");
}

#[test]
fn split_io_acl_drop_no_reply() {
    assert_acl_drop("split_io", "udp");
}

#[test]
fn sync_acl_refuse() {
    assert_acl_refuse("sync", "udp");
}

#[test]
fn split_io_acl_refuse() {
    assert_acl_refuse("split_io", "udp");
}

#[test]
fn sync_acl_accept_allowlist() {
    assert_acl_accept_allowlist("sync", "udp");
}

#[test]
fn split_io_acl_accept_allowlist() {
    assert_acl_accept_allowlist("split_io", "udp");
}

#[test]
fn sync_acl_tag_visible_to_rules() {
    assert_acl_tag_then_rule_drop("sync", "udp");
}

#[test]
fn split_io_acl_tag_visible_to_rules() {
    assert_acl_tag_then_rule_drop("split_io", "udp");
}

#[test]
fn sync_tcp_acl_drop_no_reply() {
    assert_acl_drop("sync", "tcp");
}

#[test]
fn split_io_tcp_acl_drop_no_reply() {
    assert_acl_drop("split_io", "tcp");
}

#[test]
fn sync_tcp_acl_refuse() {
    assert_acl_refuse("sync", "tcp");
}

#[test]
fn split_io_tcp_acl_refuse() {
    assert_acl_refuse("split_io", "tcp");
}

#[test]
fn sync_tcp_acl_accept_allowlist() {
    assert_acl_accept_allowlist("sync", "tcp");
}

#[test]
fn split_io_tcp_acl_accept_allowlist() {
    assert_acl_accept_allowlist("split_io", "tcp");
}

#[test]
fn sync_tcp_acl_tag_visible_to_rules() {
    assert_acl_tag_then_rule_drop("sync", "tcp");
}

#[test]
fn split_io_tcp_acl_tag_visible_to_rules() {
    assert_acl_tag_then_rule_drop("split_io", "tcp");
}

#[test]
fn sync_tcp_tier0_drop_closes_before_query_read() {
    assert_tcp_tier0_drop_closes_before_query_read("sync");
}

#[test]
fn split_io_tcp_tier0_drop_closes_before_query_read() {
    assert_tcp_tier0_drop_closes_before_query_read("split_io");
}

#[test]
fn sync_malformed_wire_dropped_without_reply() {
    let listen_port = bind_ephemeral();
    let (backend_port, _up) = mock_upstream();
    let cidr = write_cidr("malformed", "127.0.0.0/8\n");
    let yaml = acl_config(
        "sync",
        "udp",
        listen_port,
        backend_port,
        cidr.to_str().unwrap(),
        r#"
acls:
  default_action: allow
  rules: []
"#,
    );
    let (handle, metrics, _) = start_runtime(&yaml);
    let client = UdpSocket::bind("127.0.0.1:0").unwrap();
    client
        .set_read_timeout(Some(Duration::from_millis(300)))
        .unwrap();
    let target: std::net::SocketAddr = format!("127.0.0.1:{listen_port}").parse().unwrap();
    client.send_to(&[0xff, 0xfe, 0xfd], target).unwrap();
    let mut buf = [0u8; 512];
    assert!(
        client.recv_from(&mut buf).is_err(),
        "malformed must not reply"
    );
    let body = encode_builtin(metrics.builtin().gather());
    assert!(
        body.contains("conduit_parse_rejected_total"),
        "expected parse rejection metric, body:\n{body}"
    );
    handle.shutdown();
    let _ = std::fs::remove_file(cidr);
}

/// Per-listener `acls:` fully replaces global — omit would inherit admit-all.
fn assert_per_listener_replace_deny(runtime: &str) {
    let listen_port = bind_ephemeral();
    let (backend_port, _up) = mock_upstream();
    let cidr = write_cidr("replace", "10.0.0.0/8\n");
    let yaml = format!(
        r#"
schema_version: 1
dataplane:
  runtime: {runtime}
  policy_workers: 1
  io_workers: 1
listeners:
  threads: 1
  reuse_port: false
  listeners:
    - address: "127.0.0.1:{listen_port}"
      protocol: udp
      name: public
      acls:
        default_action: deny
        rules: []
forward:
  outstanding_per_backend: 32
  timeout_ms: 2000
orchestrator:
  max_attempts: 1
  max_txn_duration_ms: 5000
  txn_table_capacity: 16
events:
  queue_depth: 64
  drop_policy: drop_oldest
  sinks: []
metrics:
  enabled: true
  profile: full
data_sources:
  - name: nets
    type: cidr
    path: {cidr_path}
# Global would admit everyone; listener replace must win.
acls:
  default_action: allow
  rules: []
pools:
  - name: default
    backends:
      - address: "127.0.0.1:{backend_port}"
        weight: 100
control:
  listen_address: "127.0.0.1:0"
"#,
        cidr_path = cidr.to_str().unwrap(),
    );
    let (handle, metrics, _) = start_runtime(&yaml);
    assert!(
        send_and_recv(listen_port, 10).is_none(),
        "{runtime}: per-listener replace deny must drop (not inherit global allow)"
    );
    let body = encode_builtin(metrics.builtin().gather());
    assert!(
        body.contains(r#"action="drop""#),
        "{runtime}: expected drop metric, body:\n{body}"
    );
    handle.shutdown();
    let _ = std::fs::remove_file(cidr);
}

/// Hot-reload must apply a newly added per-listener ACL without process restart.
fn assert_per_listener_acl_hot_reload(runtime: &str) {
    let listen_port = bind_ephemeral();
    let (backend_port, _up) = mock_upstream();
    let cidr = write_cidr("reload", "10.0.0.0/8\n");
    let cidr_path = cidr.to_str().unwrap().to_string();
    let base_yaml = format!(
        r#"
schema_version: 1
dataplane:
  runtime: {runtime}
  policy_workers: 1
  io_workers: 1
listeners:
  threads: 1
  reuse_port: false
  listeners:
    - address: "127.0.0.1:{listen_port}"
      protocol: udp
      name: public
forward:
  outstanding_per_backend: 32
  timeout_ms: 2000
orchestrator:
  max_attempts: 1
  max_txn_duration_ms: 5000
  txn_table_capacity: 16
events:
  queue_depth: 64
  drop_policy: drop_oldest
  sinks: []
metrics:
  enabled: true
  profile: full
data_sources:
  - name: nets
    type: cidr
    path: {cidr_path}
acls:
  default_action: allow
  rules: []
pools:
  - name: default
    backends:
      - address: "127.0.0.1:{backend_port}"
        weight: 100
control:
  listen_address: "127.0.0.1:0"
"#
    );
    let (handle, _, store) = start_runtime(&base_yaml);
    assert!(
        send_and_recv(listen_port, 11).is_some(),
        "{runtime}: baseline without per-listener ACL must admit"
    );

    let reloaded = format!(
        r#"
schema_version: 1
dataplane:
  runtime: {runtime}
  policy_workers: 1
  io_workers: 1
listeners:
  threads: 1
  reuse_port: false
  listeners:
    - address: "127.0.0.1:{listen_port}"
      protocol: udp
      name: public
      acls:
        default_action: deny
        rules: []
forward:
  outstanding_per_backend: 32
  timeout_ms: 2000
orchestrator:
  max_attempts: 1
  max_txn_duration_ms: 5000
  txn_table_capacity: 16
events:
  queue_depth: 64
  drop_policy: drop_oldest
  sinks: []
metrics:
  enabled: true
  profile: full
data_sources:
  - name: nets
    type: cidr
    path: {cidr_path}
acls:
  default_action: allow
  rules: []
pools:
  - name: default
    backends:
      - address: "127.0.0.1:{backend_port}"
        weight: 100
control:
  listen_address: "127.0.0.1:0"
"#
    );
    let cfg = load_yaml(&reloaded).expect("reload load");
    assert!(validate(&cfg).ok, "{:?}", validate(&cfg).errors);
    // Mirror production install_validated: stamp snap.generation before swap so
    // AclGate (and other generation-keyed consumers) recompile.
    let mut snap = RuntimeSnapshot::from_config(cfg);
    snap.generation = store.generation() + 1;
    store.swap(snap);
    // Worker must notice the new generation on the next query.
    thread::sleep(Duration::from_millis(20));
    assert!(
        send_and_recv(listen_port, 12).is_none(),
        "{runtime}: after snapshot swap, per-listener deny must apply without restart"
    );
    handle.shutdown();
    let _ = std::fs::remove_file(cidr);
}

fn assert_acl_refuse_ipv6(runtime: &str) {
    let sock = match UdpSocket::bind("[::1]:0") {
        Ok(s) => s,
        Err(e) => {
            eprintln!("skipping IPv6 ACL test: cannot bind ::1 ({e})");
            return;
        }
    };
    let listen_port = sock.local_addr().unwrap().port();
    drop(sock);
    let (backend_port, _up) = mock_upstream();
    let cidr = write_cidr("v6", "::1/128\n");
    let yaml = format!(
        r#"
schema_version: 1
dataplane:
  runtime: {runtime}
  policy_workers: 1
  io_workers: 1
listeners:
  threads: 1
  reuse_port: false
  listeners:
    - address: "[::1]:{listen_port}"
      protocol: udp
      name: acl-v6
forward:
  outstanding_per_backend: 32
  timeout_ms: 2000
orchestrator:
  max_attempts: 1
  max_txn_duration_ms: 5000
  txn_table_capacity: 16
events:
  queue_depth: 64
  drop_policy: drop_oldest
  sinks: []
metrics:
  enabled: true
  profile: full
data_sources:
  - name: nets
    type: cidr
    path: {cidr_path}
acls:
  default_action: allow
  rules:
    - match: nets
      action: refuse
pools:
  - name: default
    backends:
      - address: "127.0.0.1:{backend_port}"
        weight: 100
control:
  listen_address: "127.0.0.1:0"
"#,
        cidr_path = cidr.to_str().unwrap(),
    );
    let (handle, _, _) = start_runtime(&yaml);
    let client = UdpSocket::bind("[::1]:0").expect("client ::1");
    client
        .set_read_timeout(Some(Duration::from_millis(400)))
        .unwrap();
    let target: std::net::SocketAddr = format!("[::1]:{listen_port}").parse().unwrap();
    client.send_to(&sample_query(20), target).unwrap();
    let mut buf = [0u8; 512];
    let (len, _) = client.recv_from(&mut buf).expect("REFUSED over IPv6");
    let msg = Message::from_bytes(&buf[..len]).expect("parse");
    assert_eq!(msg.response_code(), ResponseCode::Refused);
    assert_no_txn_slot_consumed(&handle, &format!("{runtime} ipv6 refuse"));
    handle.shutdown();
    let _ = std::fs::remove_file(cidr);
}

/// Hold the only txn slot on an admit listener, then refuse on a sibling listener.
/// REFUSED must still be produced (ACL refuse never acquires a slot).
fn assert_refuse_while_slot_pool_full(runtime: &str) {
    let hold_port = bind_ephemeral();
    let refuse_port = bind_ephemeral();
    // Upstream that accepts the query then sleeps before answering — keeps the
    // admit path's txn slot occupied for the window we probe refuse-without-slot.
    let slow = UdpSocket::bind("127.0.0.1:0").unwrap();
    let backend_port = slow.local_addr().unwrap().port();
    let _slow_handle = thread::spawn(move || {
        let mut buf = [0u8; 512];
        let Ok((len, peer)) = slow.recv_from(&mut buf) else {
            return;
        };
        thread::sleep(Duration::from_secs(3));
        let mut resp = buf[..len].to_vec();
        if resp.len() >= 3 {
            resp[2] = 0x81;
            resp[3] = 0x80;
        }
        let _ = slow.send_to(&resp, peer);
    });
    let cidr = write_cidr("slot-hold", "127.0.0.0/8\n");
    let yaml = format!(
        r#"
schema_version: 1
dataplane:
  runtime: {runtime}
  policy_workers: 1
  io_workers: 1
listeners:
  threads: 1
  reuse_port: false
  listeners:
    - address: "127.0.0.1:{hold_port}"
      protocol: udp
      name: hold
      acls:
        default_action: allow
        rules: []
    - address: "127.0.0.1:{refuse_port}"
      protocol: udp
      name: refuse
      acls:
        default_action: allow
        rules:
          - match: nets
            action: refuse
forward:
  outstanding_per_backend: 32
  timeout_ms: 5000
orchestrator:
  max_attempts: 1
  max_txn_duration_ms: 8000
  txn_table_capacity: 1
events:
  queue_depth: 64
  drop_policy: drop_oldest
  sinks: []
metrics:
  enabled: true
  profile: full
data_sources:
  - name: nets
    type: cidr
    path: {cidr_path}
acls:
  default_action: allow
  rules: []
pools:
  - name: default
    backends:
      - address: "127.0.0.1:{backend_port}"
        weight: 100
control:
  listen_address: "127.0.0.1:0"
"#,
        cidr_path = cidr.to_str().unwrap(),
    );
    let (handle, _, _) = start_runtime(&yaml);

    let hold_client = UdpSocket::bind("127.0.0.1:0").unwrap();
    hold_client
        .set_read_timeout(Some(Duration::from_millis(50)))
        .unwrap();
    let hold_target: std::net::SocketAddr = format!("127.0.0.1:{hold_port}").parse().unwrap();
    hold_client.send_to(&sample_query(30), hold_target).unwrap();
    wait_slots_in_use(&handle, 1, Duration::from_secs(2));
    assert_eq!(handle.txn_store.exhaustion_total(), 0);

    let msg = send_and_recv(refuse_port, 31).expect("REFUSED while pool full");
    assert_eq!(msg.response_code(), ResponseCode::Refused);
    assert_eq!(
        handle.txn_store.in_use(),
        1,
        "{runtime}: refuse must not steal or release the held slot"
    );
    assert_eq!(
        handle.txn_store.exhaustion_total(),
        0,
        "{runtime}: refuse must not attempt acquire when pool is full"
    );

    handle.shutdown();
    let _ = std::fs::remove_file(cidr);
}

/// Socket peer is allowed; ECS carries a prefix that would be dropped if ACL used it.
fn assert_acl_ignores_ecs(runtime: &str) {
    let listen_port = bind_ephemeral();
    let (backend_port, _up) = mock_upstream();
    let cidr = write_cidr("ecs", "10.0.0.0/8\n");
    let yaml = acl_config(
        runtime,
        "udp",
        listen_port,
        backend_port,
        cidr.to_str().unwrap(),
        r#"
acls:
  default_action: allow
  rules:
    - match: nets
      action: drop
"#,
    );
    let (handle, metrics, _) = start_runtime(&yaml);
    let client = UdpSocket::bind("127.0.0.1:0").unwrap();
    client
        .set_read_timeout(Some(Duration::from_millis(400)))
        .unwrap();
    let target: std::net::SocketAddr = format!("127.0.0.1:{listen_port}").parse().unwrap();
    let wire = sample_query_with_ecs(40, IpAddr::V4(Ipv4Addr::new(10, 1, 2, 3)), 24);
    client.send_to(&wire, target).unwrap();
    let mut buf = [0u8; 512];
    let (len, _) = client
        .recv_from(&mut buf)
        .expect("peer allowlist must admit despite ECS in 10/8");
    let msg = Message::from_bytes(&buf[..len]).expect("parse");
    assert_ne!(
        msg.response_code(),
        ResponseCode::Refused,
        "{runtime}: ECS must not drive ACL refuse"
    );
    let body = encode_builtin(metrics.builtin().gather());
    assert!(
        !body.contains(r#"action="drop""#),
        "{runtime}: ECS must not drive ACL drop, body:\n{body}"
    );
    handle.shutdown();
    let _ = std::fs::remove_file(cidr);
}

#[test]
fn sync_per_listener_replace_deny() {
    assert_per_listener_replace_deny("sync");
}

#[test]
fn split_io_per_listener_replace_deny() {
    assert_per_listener_replace_deny("split_io");
}

#[test]
fn sync_per_listener_acl_hot_reload() {
    assert_per_listener_acl_hot_reload("sync");
}

#[test]
fn split_io_per_listener_acl_hot_reload() {
    assert_per_listener_acl_hot_reload("split_io");
}

#[test]
fn sync_acl_refuse_ipv6() {
    assert_acl_refuse_ipv6("sync");
}

#[test]
fn split_io_acl_refuse_ipv6() {
    assert_acl_refuse_ipv6("split_io");
}

#[test]
fn split_io_acl_refuse_while_slot_pool_full() {
    // Sync holds the SharedTxnStore mutex for the entire pipeline (including
    // forward wait), so a second thread cannot observe in_use>0 mid-query.
    // split_io parks IoWait without holding that mutex — this is the runtime
    // where "refuse with pool full" is observable.
    assert_refuse_while_slot_pool_full("split_io");
}

#[test]
fn sync_acl_ignores_ecs_client_subnet() {
    assert_acl_ignores_ecs("sync");
}

#[test]
fn split_io_acl_ignores_ecs_client_subnet() {
    assert_acl_ignores_ecs("split_io");
}
