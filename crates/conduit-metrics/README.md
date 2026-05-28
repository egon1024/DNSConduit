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
| `conduit_forward_duration_seconds` | `pool`, `backend` | no |

Histogram `le` boundaries are cumulative upper bounds in seconds (Prometheus convention). Fixed bands instead of exponential doubling:

- **Phase** (`conduit_phase_duration_seconds`): 100 µs, 1 ms, 10 ms, 50 ms, 100 ms, 500 ms, 1 s, 5 s, 10 s
- **Forward RTT** (`conduit_forward_duration_seconds`): 1 ms, 10 ms, 50 ms, 100 ms, 500 ms, 1 s, 5 s, 10 s

Use `histogram_quantile()` in PromQL for percentiles; subtract adjacent `le` buckets for counts in a single band.
| `conduit_retries_total` | `pool` | no |

Per-sink event export counters (`conduit_events_*`) are included at scrape time from `EventHub` snapshots (not incremented on workers).

### User metrics (Rhai)

Rhai `metric_inc` / `metric_inc_labels` flush into the export registry after each successful hook. Series are prefixed `conduit_user_<name>`.

Current behavior uses a shared HELP string (`"Rhai user-defined metric"`) for all Rhai user counters.

Planned future enhancement: allow Rhai-defined metrics to provide per-metric HELP text so operators and dashboards can expose richer metric context.

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

When `metrics.otel.endpoint` is set, a background task pushes **counter** series to the OTLP HTTP metrics endpoint on `push_interval_ms`. Counters are derived from the same Prometheus text as scrape (histograms are not exported to OTLP yet). Use **Prometheus scrape** for full built-in series including histograms.
