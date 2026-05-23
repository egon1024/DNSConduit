# AGENTS.md

## Cursor Cloud specific instructions

### Project overview

DNSConduit is a programmable DNS proxy/forwarder written in Rust (edition 2021, stable toolchain). It is a Cargo workspace with 6 crates under `crates/`. There are no external service dependencies (no Docker, databases, or containers). Protobuf compilation is handled by the vendored `protoc-bin-vendored` crate — no system `protoc` install is needed.

### Build, lint, and test

All standard commands are in the `Makefile`:

- `make test` — runs `fmt-check`, `clippy`, and `unit` (CI parity)
- `make build` — `cargo build --workspace`
- `make fmt` — apply rustfmt
- `make clippy` — clippy with `-D warnings`
- `make unit` — `cargo test --workspace`

### Running the application

Start the binary with a YAML config file:

```
RUST_LOG=info cargo run --bin conduit -- tests/fixtures/config/minimal.yaml
```

This starts the DNS listener (default `127.0.0.1:5353`) and gRPC control plane (default `127.0.0.1:5199`). DNS forwarding requires an upstream backend to be running at the configured pool address (e.g. `127.0.0.1:5300`); the control plane works independently.

### Testing the gRPC control plane

Use `grpcurl` (or any gRPC client) with the proto import path:

```
grpcurl -plaintext -import-path proto -proto conduit/v1/control.proto 127.0.0.1:5199 conduit.v1.ConduitControl/Health
```

Available RPCs: `Health`, `GetConfig`, `ValidateConfig`, `ExportConfig`.

### Gotchas

- The `e2e_udp` integration test is `#[ignore]`d because it requires an external mock DNS backend on `127.0.0.1:5300`. All other tests run without external dependencies.
- Test fixtures live in `tests/fixtures/config/` at the workspace root.
- The `protoc-bin-vendored` crate downloads a vendored protoc binary; the first build takes longer while crates.io dependencies are fetched and compiled.
