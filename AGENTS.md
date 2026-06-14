# AGENTS.md

## Cursor Cloud specific instructions

DNS Conduit is a Rust workspace (no database, no Docker, no Node). It is a DNS
forwarder + observability platform. The single runtime binary is `conduit`;
supporting binaries are `conduitctl` (control-plane CLI) and
`conduit-dnstap-tap` (dev dnstap collector).

### Lint / test / build (CI parity)

Standard commands live in the `Makefile` and `.github/workflows/ci.yml`:

- `make test` runs `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, and `cargo test --workspace` (the exact CI sequence).
- `make build` (or `cargo build --workspace`) builds everything. No external services are needed for build/lint/test.

### Running the product end-to-end

The `conduit` binary takes a single argument: the path to a YAML config file
(NOT the binary path — passing the binary yields `stream did not contain valid
UTF-8`). It forwards client DNS queries to the upstream resolvers listed under
`pools[].backends[].address`.

- For a self-contained E2E run with no extra services, point a backend directly at a public resolver (e.g. `1.1.1.1:53`) instead of the lab's `dnsmasq` mock. Outbound DNS to public resolvers works in this environment.
- Then query it: `dig @127.0.0.1 -p <listener_port> example.com A`.
- Optional features are embedded in the `conduit` process and only activate when their YAML section is present: `control:` (gRPC control plane), `metrics.prometheus:` (scrape endpoint), and `events.sinks` of `type: dnstap` (event export).
- dnstap sinks connect OUTBOUND to a collector that must already be listening, so start `conduit-dnstap-tap -u <socket> -f json` BEFORE `conduit`.
- `conduitctl` talks to the control plane over gRPC (default `http://127.0.0.1:5199`, override with `--endpoint` or `CONDUIT_CONTROL`). Useful smoke check: `conduitctl export`.

`dnsmasq`, `grpcurl`, and `ss` are referenced by `tests/manual/*.md` but are NOT
installed here; they are only needed for the full multi-terminal lab. Basic
forwarding E2E only needs `dig` (present) plus a reachable upstream.
