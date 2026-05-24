//! Tap accepts Conduit-style bidirectional sessions and decodes extras.

use conduit_observation::fstrm::connect_bidirectional;
use std::os::unix::net::UnixStream;
use std::process::{Command, Stdio};
use std::thread;
use std::time::Duration;

#[test]
fn tap_prints_extra_json_on_stdout() {
    let dir = tempfile::tempdir().unwrap();
    let sock = dir.path().join("tap.sock");
    let tap_bin = env!("CARGO_BIN_EXE_conduit-dnstap-tap");

    let child = Command::new(tap_bin)
        .args(["-u", sock.to_str().unwrap(), "-f", "json", "--once"])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn tap");

    thread::sleep(Duration::from_millis(200));

    let client = UnixStream::connect(&sock).expect("connect");
    let mut writer = connect_bidirectional(client, "protobuf:dnstap.Dnstap").expect("handshake");

    let payload = sample_dnstap_with_extra(br#"{"pool":"default","backend":"10.0.0.1:53"}"#);
    writer.write_data_frame(&payload).expect("write");
    writer.finish().expect("finish");

    let output = child.wait_with_output().expect("wait");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("pool") && stdout.contains("default"),
        "stdout should contain extra pool: {stdout}"
    );
}

fn sample_dnstap_with_extra(extra_json: &[u8]) -> Vec<u8> {
    let mut msg = Vec::new();
    write_tag(&mut msg, 1, 0);
    write_varint(&mut msg, 6);

    let mut dnstap = Vec::new();
    write_tag(&mut dnstap, 1, 2);
    write_bytes(&mut dnstap, b"conduit-dev");
    write_tag(&mut dnstap, 3, 2);
    write_bytes(&mut dnstap, extra_json);
    write_tag(&mut dnstap, 15, 0);
    write_varint(&mut dnstap, 1);
    write_tag(&mut dnstap, 14, 2);
    write_bytes(&mut dnstap, &msg);
    dnstap
}

fn write_tag(out: &mut Vec<u8>, field: u32, wire: u8) {
    write_varint(out, ((field << 3) | u32::from(wire)) as u64);
}

fn write_varint(out: &mut Vec<u8>, mut n: u64) {
    while n >= 0x80 {
        out.push((n as u8) | 0x80);
        n >>= 7;
    }
    out.push(n as u8);
}

fn write_bytes(out: &mut Vec<u8>, data: &[u8]) {
    write_varint(out, data.len() as u64);
    out.extend_from_slice(data);
}
