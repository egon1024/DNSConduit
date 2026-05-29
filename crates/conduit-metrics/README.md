# conduit-metrics

Phase 4/4b metrics export and pipeline tracing for DNS Conduit.

## Metrics

### Config

```yaml
metrics:
  enabled: true
  profile: full          # full | minimal | off
  prometheus:
    listen_address: "127.0.0.1:9090"
    path: /metrics
  otel:
    endpoint: "http://127.0.0.1:4318/v1/metrics"
    push_interval_ms: 15000
    resource_attributes:
      service.name: conduit
```

When the `metrics` section is **omitted**, export is disabled (no scrape listener, no hot-path increments).

### Profile: minimal vs full

| Series | Hot path `minimal` | Hot path `full` | Scrape only |
|--------|-------------------|-----------------|-------------|
| `conduit_queries_total` | `listener`, `protocol` | + `qtype`, `qclass`, `ip_family` | — |
| `conduit_queries_by_pool_total` | yes (`pool`) | yes | — |
| `conduit_parse_rejected_total` | no | yes (`reason`) | — |
| `conduit_responses_total` | no | yes (`listener`, `protocol`, `rcode_class`, `ip_family`) | — |
| Phase 4 forward/phase/retry histograms & counters | no | no (unchanged) | — |
| `conduit_forward_outstanding` | — | — | yes |
| `conduit_pool_backends_configured` | — | — | yes |
| `conduit_build_info`, `conduit_start_time_seconds`, `conduit_config_generation` | — | — | yes |
| `conduit_process_resident_bytes`, `conduit_process_open_fds` | — | — | yes (`full` only, Linux `/proc`) |

**Cardinality policy:** built-in labels never include `qname`, client IP, or `txn_id`. Use dnstap/events or tracing for per-name detail.

### Built-in series (phase 4 + 4b)

| Name | Labels | Notes |
|------|--------|-------|
| `conduit_queries_total` | see profile table | Incremented after successful parse |
| `conduit_queries_by_pool_total` | `pool` | After route selection |
| `conduit_parse_rejected_total` | `reason` | `empty`, `wire_error`, `not_query`, `no_question`, `multi_question` |
| `conduit_responses_total` | `listener`, `protocol`, `rcode_class`, `ip_family` | `rcode_class`: NOERROR, NXDOMAIN, SERVFAIL, OTHER |
| `conduit_phase_duration_seconds` | `phase` | `full` profile only |
| `conduit_forward_*`, `conduit_retries_total` | (phase 4) | `full` profile only |
| `conduit_build_info` | `version`, `revision`, `dirty`, `profile` | Scrape-only; value `1`. See [Build metadata](#build-metadata). |
| `conduit_start_time_seconds` | — | Unix timestamp when the process started |
| `conduit_config_generation` | — | Active config generation (from scrape snapshot) |

Histogram `le` boundaries are cumulative upper bounds in seconds (Prometheus convention):

- **Phase:** 100 µs, 1 ms, 10 ms, 50 ms, 100 ms, 500 ms, 1 s, 5 s, 10 s
- **Forward RTT:** 1 ms, 10 ms, 50 ms, 100 ms, 500 ms, 1 s, 5 s, 10 s

Use `histogram_quantile()` in PromQL for percentiles.

**PromQL examples:**

```promql
sum(rate(conduit_queries_total[5m])) by (listener, protocol)
sum(rate(conduit_queries_by_pool_total[5m])) by (pool)
sum(rate(conduit_parse_rejected_total[5m])) by (reason)
sum(rate(conduit_responses_total[5m])) by (rcode_class)
time() - conduit_start_time_seconds
conduit_config_generation
conduit_build_info{revision="abc1234",dirty="true",profile="debug"}
```

### Build metadata

`conduit_build_info` is set at **compile time** via `build.rs` (not at runtime):

| Label | Meaning |
|-------|---------|
| `version` | Workspace semver from `Cargo.toml` (`CARGO_PKG_VERSION`) |
| `revision` | Short git commit (`git rev-parse --short HEAD`), or `unknown` when not built from a git checkout |
| `dirty` | `true` if the working tree had uncommitted changes at build time (`git status --porcelain`); otherwise `false` |
| `profile` | Cargo profile for this binary (`debug` or `release`) |

Rebuild after pulling or editing sources so `revision` and `dirty` reflect the binary you are running. Release CI builds typically show `dirty="false"` and `profile="release"`.

Per-sink event export counters (`conduit_events_*`) are included at scrape time from `EventHub` snapshots (not incremented on workers).

### User metrics (Rhai)

Rhai `metric_inc` / `metric_inc_labels` flush into the export registry after each successful hook. Series are prefixed `conduit_user_<name>`.

### Metrics-path parity (Prometheus + OTEL)

Defined export paths are **Prometheus scrape** and **OTEL push**. Both consume the same Prometheus text from `render_prometheus()`. Counters and gauges from 4b built-ins are exported to OTLP; histograms are mapped with the same bucket upper bounds as Prometheus `le` labels.

**Policy:** changes that add or modify a built-in on one defined path must update all defined paths in the same change (or document an explicit exception here).

**Intentional exceptions:** none for phase 4b core families.

## Tracing

```yaml
tracing:
  enabled: true
  activation:
    tag: trace
    selectors:
      - type: qtype
        value: A
    sample_rate: 1.0
  output:
    log_json: false
```

Default: **off** — no `TraceLog` allocation on the hot path.

Activation uses the same selector types and `hash_sample(txn_id, rate)` as event sink filters (phase 2.7). Evaluated after **RequestRules**.

Completed traces are stored in a bounded in-memory `TraceStore` (1000 entries, 5 minute TTL) for gRPC `GetTrace`.

## OTEL push

When `metrics.otel.endpoint` is set, a background task pushes metrics derived from the same Prometheus text as scrape on `push_interval_ms`. Use Prometheus scrape when you need ad-hoc PromQL on histogram `_bucket` series; OTLP carries counters, gauges, and histogram summaries for built-ins.
