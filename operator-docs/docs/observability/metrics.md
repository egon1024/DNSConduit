# Metrics

Built-in Prometheus metrics for the [dataplane](/glossary/index.md#dataplane): query volume, pool mix, forward health, pipeline timing, and process gauges. They are separate from the DNS query path — export backlog does not delay client responses.

## Enabling export

When the **`metrics:`** section is **omitted** from your [config file](/control-plane/config-file.md), built-in export is **off** — no scrape listener and no hot-path recording.

To enable metrics, add a block with **`enabled: true`** and at least one export path (Prometheus scrape and/or OTEL push):

```yaml
metrics:
  enabled: true
  profile: full          # minimal | full | off
  prometheus:
    listen_address: "127.0.0.1:9090"
    path: /metrics
  otel:
    endpoint: "http://127.0.0.1:4318/v1/metrics"
    push_interval_ms: 15000
```

| Setting | Meaning |
|---------|---------|
| `metrics.enabled` | Must be `true` for built-in recording and export |
| `metrics.profile` | **`minimal`** or **`full`** (default **`full`** when the block is present). **`off`** disables export even if `enabled` is true |
| `metrics.prometheus` | Optional HTTP scrape listener |
| `metrics.otel` | Optional OTLP HTTP push on an interval |

YAML field reference: [Config schema: metrics and tracing](/reference/config-schema/metrics-and-tracing.md).

## Profiles

**`minimal`** keeps hot-path cardinality low (query volume and per-pool counters). **`full`** adds parse failures, response codes, forward latency, phase histograms, retries, and Linux process gauges.

Both profiles use the same export paths — profile chooses **what** is recorded, not **how** you scrape or push. Trade-offs and the full series table: [Built-in metrics — Profiles](/observability/built-in-metrics.md#profiles). Walkthrough: [Operator metrics profiles](/guides/operator-metrics-profiles.md).

## Where metrics fit the query path

Counters and histograms attach to [pipeline phases](/concepts/architecture-and-packet-path.md#pipeline-phases) ([Parse](/concepts/architecture-and-packet-path.md#parse) through [Send](/concepts/architecture-and-packet-path.md#send)). The architecture page links each phase to the relevant series.

Exhaustive list — every built-in name, label, and **when** it increments: **[Built-in metrics](/observability/built-in-metrics.md)**.

Built-in labels never include `qname`, client IP, or transaction id. Use [Event export](/observability/event-export.md) or [Tracing](/observability/tracing.md) for per-query detail.

## Related topics

- [Built-in metrics](/observability/built-in-metrics.md) — series reference and PromQL examples
- [Architecture and packet path](/concepts/architecture-and-packet-path.md) — phase-by-phase metric hooks
- [Operator metrics profiles](/guides/operator-metrics-profiles.md) — lab validation of `minimal` vs `full`
- [Metrics and tracing](/guides/metrics-and-tracing.md) — end-to-end observability setup (guide in progress)
