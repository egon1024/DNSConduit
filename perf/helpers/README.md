# Perf harness helpers

Shipped companion lab binaries live in the Rust workspace (same packaging class as
`conduit-dnstap-tracer`).

| Binary | Crate | Role |
|--------|-------|------|
| `conduit-otlp-metrics-tracer` | `crates/conduit-otlp-metrics-tracer` | OTLP HTTP `/v1/metrics` lab receiver for `feature_tax` OTLP scenarios |

Scrape-only metrics scenarios do not require the OTLP receiver. Build from a source
checkout with `cargo build -p conduit-otlp-metrics-tracer --release`, or use the
prebuilt companion from release assets when available.
