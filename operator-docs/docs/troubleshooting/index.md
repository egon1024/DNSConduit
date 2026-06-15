# Troubleshooting

Symptom-oriented pointers for common operator issues. Each section links to the canonical topic page — this hub does not duplicate full configuration reference.

## Observability { #observability }

### Metrics scrape returns connection refused or empty

| Symptom | Likely cause | What to check |
|---------|--------------|---------------|
| `curl` to `/metrics` fails with connection refused | `metrics.prometheus` not configured, metrics disabled, or Conduit not listening on that address | Confirm `metrics.enabled: true` and `metrics.prometheus.listen_address` in the **startup** config; [Metrics](/observability/metrics.md) |
| Connection works but no `conduit_*` series | `metrics:` omitted or `enabled: false` | Built-ins are off when the block is missing — [Metrics — Enabling export](/observability/metrics.md#enabling-export) |
| Scrape works but counters stay at zero | No DNS traffic yet, or wrong listener port in `dig` | Send a query to the configured listener; check [`conduit_queries_total`](/observability/built-in-metrics.md#conduit_queries_total) |
| Changed scrape address or enabled metrics after start — still old behavior | Export listeners bind at **process start** | **Restart** Conduit after `metrics:` changes — [Observability — Changing observability config](/observability/index.md#changing-observability-config) |

Smoke test:

```bash
curl -sS "http://127.0.0.1:9090/metrics" | head
dig @127.0.0.1 -p 15353 +time=3 example.com A
curl -sS "http://127.0.0.1:9090/metrics" | grep conduit_queries
```

Adjust host, scrape port, and listener port to your config.

### OTEL metrics push failures

| Symptom | Likely cause | What to check |
|---------|--------------|---------------|
| Log line `failed to build OTLP metric exporter` at startup | Invalid `metrics.otel.endpoint` (not `http://` or `https://`) | [Metrics — Export architecture](/observability/metrics.md#export-architecture); validate with `conduitctl validate --file` |
| Periodic `otel metrics push failed` at **`warn`** | Collector down, TLS verify failure, or network block | Endpoint reachable; for self-signed HTTPS use `allow_invalid_certs: true` (lab only) or fix collector cert |
| No push logs | Push interval default **15s**; successes log at **`debug`** only | Set `logging.level: debug` briefly to see `otel metrics push ok` |
| Enabled OTEL after process start — no push | OTEL task starts at **process start** | **Restart** after adding or changing `metrics.otel` |

Authentication headers for collectors are **not** operator-supported yet. Bind Prometheus scrape to loopback or restrict with firewall — scrape has **no built-in auth** today.

### Event export / dnstap gaps

| Symptom | Likely cause | What to check |
|---------|--------------|---------------|
| No frames at collector | Collector not running, wrong socket path, or sink filters exclude the query | Start **`conduit-dnstap-tracer`** before Conduit; see [Event export and dnstap](/guides/event-export-dnstap.md) |
| `conduit_events_queue_dropped_total` increasing | Collector slow or down; queue full | [Event export — Overload and metrics](/observability/event-export.md#overload-and-metrics); fix collector throughput |
| Added a new sink via reload — no effect | New sinks require **restart** | [Event export — Changing events config](/observability/event-export.md#changing-events-config) |
| Query frames missing pool/backend on `query` emit | Expected — pool/backend filters apply to **`response`** / **`retry`** only | [Event export — Filters](/observability/event-export.md#filters) |

### Pipeline trace not found

| Symptom | Likely cause | What to check |
|---------|--------------|---------------|
| `conduitctl trace N` — not found | Wrong `txn_id`, trace expired, or activation did not match | [Tracing — Activation](/observability/tracing.md#activation); traces TTL **5 minutes**, store cap **1000** |
| Control plane unavailable | No `control:` at startup | `conduitctl trace` needs gRPC — [gRPC and conduitctl](/control-plane/grpc-and-conduitctl.md) |
| Unsure of `txn_id` | Id is per-worker and increments | Set `logging.level: debug`, send query, read `txn_id` from **`query complete`** — [Logging](/observability/logging.md#per-query-summaries) |
| Enabled tracing after start — no traces | Tracing compiled at **process start** | **Restart** after `tracing:` changes — [Tracing — Changing tracing config](/observability/tracing.md#changing-tracing-config) |

### Logging surprises

| Symptom | Likely cause | What to check |
|---------|--------------|---------------|
| No per-query lines at default level | **`query complete`** is **`debug`** only | By design — use [Metrics](/observability/metrics.md) for volume; enable **`debug`** briefly for `txn_id` |
| `RUST_LOG` overrides config | Env set at startup | Unset `RUST_LOG` in production — [Logging — RUST_LOG override](/observability/logging.md#rust_log-override) |
| Changed `logging.level` via reload — no effect | Subscriber binds at **process start** | **Restart** after logging changes — [Logging — Changing logging config](/observability/logging.md#changing-logging-config) |

## Related topics

- [Observability](/observability/index.md) — which signal to use, OTEL naming, reload matrix
- [Metrics and tracing](/guides/metrics-and-tracing.md) — metrics + tracing lab
- [Event export and dnstap](/guides/event-export-dnstap.md) — dnstap lab
- [Operator metrics profiles](/guides/operator-metrics-profiles.md) — minimal vs full lab
- [Control plane workflows](/guides/control-plane-workflows.md) — reload and apply (in progress)
