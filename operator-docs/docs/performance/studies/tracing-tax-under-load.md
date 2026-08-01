# Pipeline tracing tax

What does enabling full pipeline tracing cost under [`forward_fast`](/performance/methodology.md#load-shapes)?

Numbers are same-host comparisons (relative to baselines measured on one named
lab profile) and are **not** service-level objectives. See the
[performance hub disclaimer](/performance/index.md).

## When this matters

[Pipeline tracing](/observability/tracing.md) records per-query phase detail for
diagnosis. Unlike process [logging](/observability/logging.md) levels, tracing
is a dedicated [`tracing:`](/reference/config-schema/metrics-and-tracing.md)
block with selectors and sampling. This study compares observability off against
tracing enabled for **qtype A at 100% sample** under
[`forward_fast`](/performance/methodology.md#load-shapes) (dnsperf query file is
A-heavy).

## What we varied

- **Varied:** tracing posture
  ([off via `metrics_off`](/performance/scenarios.md#feature-tax-metrics-off-forward-fast) vs
  [`tracing_on`](/performance/scenarios.md#feature-tax-tracing-on-forward-fast))
- **Held fixed:** metrics disabled, no dnstap,
  [`sync`](/concepts/runtime-and-concurrency.md#sync-runtime-default),
  [`forward_fast`](/performance/methodology.md#load-shapes)

<!-- perf-study-evidence:start -->
## Evidence

<div class="perf-chart" markdown="1">

### Feature tax — tracing off vs on (forward_fast)

![Feature tax — tracing off vs on (forward_fast)](../generated/tracing-off-vs-on-forward-fast.svg)

[Download CSV](../generated/tracing-off-vs-on-forward-fast.csv)

| Posture | Runtime | Achieved QPS | Avg latency (ms) | Sent | Completed | Lost | Workers |
| --- | --- | --- | --- | --- | --- | --- | --- |
| [metrics_off](/performance/scenarios.md#feature-tax-metrics-off-forward-fast) | sync | 138760.8 | 2.0 | 1391423 | 1387890 | 3449 | ingress=2 |
| [tracing_on](/performance/scenarios.md#feature-tax-tracing-on-forward-fast) | sync | 71846.1 | 3.8 | 722221 | 718827 | 3455 | ingress=2 |

</div>
<!-- perf-study-evidence:end -->

## Takeaway

On this published reference, **full A-query tracing costs about half** the
achieved QPS of observability off (lab absolute ~72k vs ~139k) with higher
average latency. **Operator posture:** enable tracing for diagnosis windows;
do not leave 100% sampling on as a standing production posture. Narrow
selectors and `sample_percent` before remeasuring.

## Related guides

- [Tracing](/observability/tracing.md)
- [Reference: metrics and tracing](/reference/config-schema/metrics-and-tracing.md)
- [Logging verbosity tax](/performance/studies/logging-verbosity-tax.md)

## Member scenarios

- [feature-tax-metrics-off-forward-fast](/performance/scenarios.md#feature-tax-metrics-off-forward-fast)
- [feature-tax-tracing-on-forward-fast](/performance/scenarios.md#feature-tax-tracing-on-forward-fast)

## Related

- [Studies hub](/performance/studies/index.md)
- [Reference results](/performance/reference.md)
- [Methodology](/performance/methodology.md)
