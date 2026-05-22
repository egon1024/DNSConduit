//! Optional loopback E2E — requires a mock backend on 127.0.0.1:5300.

#[test]
#[ignore = "requires external mock DNS backend"]
fn loopback_udp_query() {
    // Manual: cargo run -p conduit -- tests/fixtures/config/dataplane-minimal.yaml
    // dig @127.0.0.1 -p 5353 example.com A
}
