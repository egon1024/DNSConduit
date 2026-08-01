# OTLP tax under load

What does OTLP metrics push cost versus observability off under [`forward_fast`](/performance/methodology.md#load-shapes)?

Numbers are same-host comparisons (relative to baselines measured on one named
lab profile) and are **not** service-level objectives. See the
[performance hub disclaimer](/performance/index.md).

## When this matters

Operators enabling [OTLP metrics push](/guides/otlp-metrics-push.md) need a
directional sense of hot-path cost versus leaving metrics off, and how that
compares to a Prometheus scrape posture. Configure push under
[`metrics.otel`](/reference/config-schema/metrics-and-tracing.md). This study
reuses existing `feature_tax` cells; the OTLP member requires the
`conduit-otlp-metrics-tracer` companion and is skip-tolerant in the harness.
Compare also the [metrics scrape ladder](/performance/studies/metrics-scrape-ladder.md).

## What we varied

- **Varied:** observability posture
  ([`metrics_off`](/performance/scenarios.md#feature-tax-metrics-off-forward-fast),
  [`metrics_otlp_push`](/performance/scenarios.md#feature-tax-metrics-otlp-push-forward-fast),
  [`metrics_standard_scrape`](/performance/scenarios.md#feature-tax-metrics-standard-scrape-forward-fast))
- **Held fixed:** [`sync`](/concepts/runtime-and-concurrency.md#sync-runtime-default) runtime,
  [`forward_fast`](/performance/methodology.md#load-shapes), same dnsperf recipe on the
  named lab profile
- **Skip rule:** if the OTLP companion is unavailable, the OTLP pole is omitted
  or marked unavailable — figures never invent series

<!-- perf-study-evidence:start -->
## Evidence

<div class="perf-chart" markdown="1">

### Feature tax — OTLP push vs baselines (forward_fast)

![Feature tax — OTLP push vs baselines (forward_fast)](../generated/otlp-tax-under-load-forward-fast.svg)

[Download CSV](../generated/otlp-tax-under-load-forward-fast.csv)

| Posture | Runtime | Achieved QPS | Avg latency (ms) | Sent | Completed | Lost | Workers |
| --- | --- | --- | --- | --- | --- | --- | --- |
| [metrics_off](/performance/scenarios.md#feature-tax-metrics-off-forward-fast) | sync | 138760.8 | 2.0 | 1391423 | 1387890 | 3449 | ingress=2 |
| [metrics_otlp_push](/performance/scenarios.md#feature-tax-metrics-otlp-push-forward-fast) | sync | 116878.8 | 2.6 | 1172470 | 1169073 | 3397 | ingress=2 |
| [metrics_standard_scrape](/performance/scenarios.md#feature-tax-metrics-standard-scrape-forward-fast) | sync | 121399.7 | 2.3 | 1217696 | 1214291 | 3449 | ingress=2 |

</div>
<!-- perf-study-evidence:end -->

## Takeaway

On this published reference, **OTLP push costs about 16%** achieved QPS versus
observability off, somewhat more than **standard scrape (~12% tax)** on the
same recipe (lab absolute ~117k / ~121k / ~139k). **Operator posture:** treat
OTLP as another export tax in the same ballpark as scrape — enable push only
when you have a collector path; otherwise prefer scrape and size from the
[metrics scrape ladder](/performance/studies/metrics-scrape-ladder.md). If the
OTLP member was skipped in the promoted reference, treat this study as incomplete
for that posture — do not read empty bars as “zero tax.”

## Related guides

- [OTLP metrics push smoke](/guides/otlp-metrics-push.md)
- [Operator metrics bases](/guides/operator-metrics-bases.md)
- [Metrics configurability](/observability/metrics-configurability.md)
- [Reference: metrics and tracing](/reference/config-schema/metrics-and-tracing.md)
- [Metrics scrape ladder](/performance/studies/metrics-scrape-ladder.md)
- [Aggressive scrape cadence](/performance/studies/metrics-scrape-hammer.md)

## Member scenarios

- [feature-tax-metrics-off-forward-fast](/performance/scenarios.md#feature-tax-metrics-off-forward-fast)
- [feature-tax-metrics-otlp-push-forward-fast](/performance/scenarios.md#feature-tax-metrics-otlp-push-forward-fast)
- [feature-tax-metrics-standard-scrape-forward-fast](/performance/scenarios.md#feature-tax-metrics-standard-scrape-forward-fast)

## Related

- [Studies hub](/performance/studies/index.md)
- [Reference results](/performance/reference.md)
- [Methodology](/performance/methodology.md)
