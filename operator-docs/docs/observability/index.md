# Observability

Metrics, tracing, event export, and logging for Conduit — how you observe the [dataplane](/glossary/index.md#dataplane) without changing query behavior.

**Start here:**

1. [Metrics](/observability/metrics.md) — enable export, **`minimal`** vs **`full`** profiles, Prometheus scrape and OTEL push
2. [Built-in metrics](/observability/built-in-metrics.md) — every built-in series, labels, pipeline mapping, PromQL examples
3. [Tracing](/observability/tracing.md) — per-query pipeline traces
4. [Event export](/observability/event-export.md) — [dnstap](/glossary/index.md#dnstap) and related sinks
5. [Logging](/observability/logging.md) — structured logs (in progress)

For a lab walkthrough of metrics profiles, see [Operator metrics profiles](/guides/operator-metrics-profiles.md). Config fields: [Reference: metrics and tracing](/reference/config-schema/metrics-and-tracing.md).
