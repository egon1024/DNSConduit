# Metrics scrape tax under split_io

<div class="study-question" markdown="1">

What does standard scrape cost versus observability off when the runtime is [`split_io`](/concepts/runtime-and-concurrency.md#split-io-runtime)?

</div>

Numbers are same-host comparisons on a single reference host and are **not**
service-level objectives. See the
[performance hub disclaimer](/performance/index.md).

## When this matters

The [metrics scrape tax](/performance/studies/metrics-scrape-ladder.md) uses
**[`sync`](/concepts/runtime-and-concurrency.md#sync-runtime-default)**. If production already runs
[`split_io`](/concepts/runtime-and-concurrency.md#split-io-runtime)
(`dataplane.runtime: split_io`), scrape tax on that model is the more relevant
comparison. This study pairs observability off and standard scrape under
`split_io` + [`forward_fast`](/performance/methodology.md#load-shapes) (2 ingress /
2 policy / 2 I/O workers).

## What we varied

- **Varied:** observability posture
  ([`metrics_off`](/performance/scenarios.md#feature-tax-metrics-off-split-io-forward-fast) vs
  [`metrics_standard_scrape`](/performance/scenarios.md#feature-tax-metrics-standard-scrape-split-io-forward-fast))
- **Held fixed:** `split_io`, ingress/policy/io = 2,
  [`forward_fast`](/performance/methodology.md#load-shapes), dnsperf recipe

<!-- perf-study-evidence:start -->
## Evidence

<div class="perf-chart" markdown="1">

### Feature tax — metrics scrape under split_io (forward_fast)

![Feature tax — metrics scrape under split_io (forward_fast)](../generated/metrics-scrape-split-io-forward-fast.svg)

[Download CSV](../generated/metrics-scrape-split-io-forward-fast.csv)

| Posture | Runtime | Achieved QPS | Avg latency (ms) | Sent | Completed | Lost | Workers |
| --- | --- | --- | --- | --- | --- | --- | --- |
| [metrics_off](/performance/scenarios.md#feature-tax-metrics-off-split-io-forward-fast) | split_io | 139604.8 | 14.3 | 1397888 | 1397888 | 0 | ingress=2, policy=2, io=2 |
| [metrics_standard_scrape](/performance/scenarios.md#feature-tax-metrics-standard-scrape-split-io-forward-fast) | split_io | 142733.8 | 14.0 | 1429317 | 1429317 | 0 | ingress=2, policy=2, io=2 |

</div>
<!-- perf-study-evidence:end -->

<!-- perf-study-deltas:start -->
## At a glance

- **metrics scrape under split_io (forward_fast):** `metrics_standard_scrape` is about **1.0×** `metrics_off` (~143k vs ~140k).
<!-- perf-study-deltas:end -->

## Takeaway

**Under `split_io`, standard scrape did not show a clear QPS tax on this
median.** Obs-off and standard scrape are within about **2%** (~143k vs ~140k).
Do not assume the sync ladder percentage transfers — remeasure on your runtime.

**What to do:** when sizing scrape on a `split_io` deployment, remeasure this
pair on your hardware. Still pick minimal vs standard from
[cardinality need](/guides/operator-metrics-bases.md).

## Related guides

- [Operator metrics bases](/guides/operator-metrics-bases.md)
- [Runtime and concurrency](/concepts/runtime-and-concurrency.md)
- [Metrics scrape tax](/performance/studies/metrics-scrape-ladder.md) (sync)
- [Aggressive scrape cadence](/performance/studies/metrics-scrape-hammer.md)

## Member scenarios

- [feature-tax-metrics-off-split-io-forward-fast](/performance/scenarios.md#feature-tax-metrics-off-split-io-forward-fast)
- [feature-tax-metrics-standard-scrape-split-io-forward-fast](/performance/scenarios.md#feature-tax-metrics-standard-scrape-split-io-forward-fast)

## Related

- [Studies hub](/performance/studies/index.md)
- [Reference results](/performance/reference.md)
- [Methodology](/performance/methodology.md)
