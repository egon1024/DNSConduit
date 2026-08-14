# AGENTS.md — DNSConduit

DNSConduit is a Rust workspace monorepo for a DNS forwarding + observability
platform. No database, no Node, no Docker required for the core dev loop.

Key binaries (workspace members under `crates/`):

- `conduit` — the DNS server. Takes a **single positional argument: the path to a
  YAML config** (defaults to `conduit.yaml` if omitted). It is **not** a
  subcommand CLI; do not pass flags before the config path.
- `conduitctl` — gRPC control-plane client. Default endpoint
  `http://127.0.0.1:5199` (override with `--endpoint` or `CONDUIT_CONTROL`).
- `conduit-dnstap-tracer` — dev/troubleshooting dnstap collector (binary is
  `conduit-dnstap-tracer`; some docs refer to it informally as the "dnstap tap").

## Toolchain

- Rust stable, pinned via `rust-toolchain.toml`.
- `requirements.txt` is **only** for building `operator-docs/` (MkDocs); it is not
  used by the Rust workspace.

## Standard dev commands

```bash
cargo build --workspace          # build everything
make test                        # CI parity: fmt-check + clippy (-D warnings) + cargo test --workspace
make fmt                         # apply rustfmt
make build                       # cargo build --workspace
```

`make test` mirrors `.github/workflows/ci.yml` exactly:
`cargo fmt --all -- --check`, then `cargo clippy --workspace --all-targets -- -D warnings`,
then `cargo test --workspace`.

## Contributions

Every commit must carry a DCO `Signed-off-by` trailer (see `CONTRIBUTING.md`);
commit with `git commit -s`.

## Cursor Cloud specific instructions

These are the non-obvious details for running/validating the product in a fresh
Cloud Agent VM (no lab hardware, no dnsmasq mock, no Docker):

- **`conduit` takes only a config path** as its argument, e.g.
  `./target/debug/conduit /path/to/config.yaml`. Passing the binary path or a
  flag as the first arg produces a config-read error.
- **Start `conduit-dnstap-tracer` before `conduit`** when exercising dnstap
  export. The tracer binds the unix socket; `conduit` connects to it as a sink.
  Example: `conduit-dnstap-tracer -u /tmp/conduit-dnstap.sock -f log`, then start
  `conduit` with a config whose event sink points at
  `unix:/tmp/conduit-dnstap.sock`.
- **Self-contained E2E without the lab's dnsmasq mock:** point a pool backend
  directly at a public resolver (`1.1.1.1:53`) in a temporary config, start
  `conduit`, then query it: `dig @127.0.0.1 -p <listener_port> example.com A`.
  Outbound DNS to public resolvers works from the Cloud VM.
- **Observability endpoints** (when enabled in config): Prometheus at
  `http://<listen_address><path>` (e.g. `http://127.0.0.1:9090/metrics`), gRPC
  control plane at `control.listen_address` (default `127.0.0.1:5199`). Scrape
  with `curl`; drive the control plane with `conduitctl` (`export`, `validate`,
  `acl check <ip>`, `health show`, `reload`, `trace <txn_id>`).
- **Manual-test lab manuals live in the private companion repo
  `egon1024/DNSConduitCursor`** (referenced by `tests/manual/*.md` stubs and
  `tests/manual/README.md`). If the Cursor GitHub App token cannot clone it
  (`git clone` → "Repository not found"; confirm with
  `gh api installation/repositories --jq '.repositories[].full_name'`), the
  driver docs are unavailable — fall back to the self-contained E2E above and the
  local config assets under `tests/manual/config/` and `packaging/examples/`.

A ready-to-run self-contained E2E config (public-resolver forwarding + metrics +
control plane + dnstap) can be assembled from
`packaging/examples/conduit.reference.yaml`.
