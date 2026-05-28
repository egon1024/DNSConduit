# conduit-metrics

Phase 4 metrics export and pipeline tracing for DNS Conduit.

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

### Built-in series (hot path when enabled)

| Name | Labels | `profile: minimal` |
|------|--------|--------------------|
| `conduit_queries_total` | `listener`, `protocol` | **yes** |
| `conduit_phase_duration_seconds` | `phase` | no |
| `conduit_forward_attempts_total` | `pool`, `backend`, `outcome` | no |
| `conduit_forward_errors_total` | `pool`, `reason` | no |
| `conduit_retries_total` | `pool` | no |

Observation per-sink counters are exported at scrape time from `ObservationHub` snapshots (not incremented on workers).

### User metrics (Rhai)

Rhai `metric_inc` / `metric_inc_labels` flush into the export registry after each successful hook. Series are prefixed `conduit_user_<name>`.

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

Activation uses the same selector types and `hash_sample(txn_id, rate)` as observation sink filters (phase 2.7). Evaluated after **RequestRules**.

Completed traces are stored in a bounded in-memory `TraceStore` (1000 entries, 5 minute TTL) for gRPC `GetTrace`.

## OTEL push

The OTEL background task runs on `push_interval_ms` and logs push ticks. Full OTLP instrument mapping is not wired in v1; use **Prometheus scrape** as the primary export path.
