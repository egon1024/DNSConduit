# Metrics collect vs emit

<div class="study-question" markdown="1">

Is metrics cost dominated by hot-path collect, or by scrape emit?

</div>

Numbers are same-host comparisons on a single reference host and are **not**
service-level objectives. See the
[performance hub disclaimer](/performance/index.md).

## When this matters

[`metrics.collection`](/observability/metrics-configurability.md) can record
hot-path series without exporting them (`collect: true`, `emit: false`), or skip
recording entirely. Operators often ask whether the tax is **recording** or
**scrape export**. This study keeps `metrics.base: standard` and varies
collect/emit on the volume/failures/lookup/timing categories under [`forward_fast`](/performance/methodology.md#load-shapes).
See also the [metrics scrape tax](/performance/studies/metrics-scrape-ladder.md)
(base off / minimal / standard with normal scrape).

## What we varied

- **Varied:** collect/emit posture
  ([`metrics_no_collect`](/performance/scenarios.md#feature-tax-metrics-no-collect-forward-fast) →
  [`metrics_collect_only`](/performance/scenarios.md#feature-tax-metrics-collect-only-forward-fast) →
  [`metrics_collect_emit`](/performance/scenarios.md#feature-tax-metrics-collect-emit-forward-fast))
- **Held fixed:** [`sync`](/concepts/runtime-and-concurrency.md#sync-runtime-default) runtime,
  [`forward_fast`](/performance/methodology.md#load-shapes), standard base, scrape listener present

<!-- perf-study-evidence:start -->
## Evidence

<div class="perf-chart" markdown="1">

### Feature tax — collect vs emit (forward_fast)

![Feature tax — collect vs emit (forward_fast)](../generated/metrics-collect-vs-emit-forward-fast.svg)

[Download CSV](../generated/metrics-collect-vs-emit-forward-fast.csv)

| Posture | Runtime | Achieved QPS | Avg latency (ms) | Sent | Completed | Lost | Workers |
| --- | --- | --- | --- | --- | --- | --- | --- |
| [metrics_no_collect](/performance/scenarios.md#feature-tax-metrics-no-collect-forward-fast) | sync | 72940.1 | 27.3 | 732015 | 732015 | 0 | ingress=2 |
| [metrics_collect_only](/performance/scenarios.md#feature-tax-metrics-collect-only-forward-fast) | sync | 69562.3 | 28.7 | 697649 | 697649 | 0 | ingress=2 |
| [metrics_collect_emit](/performance/scenarios.md#feature-tax-metrics-collect-emit-forward-fast) | sync | 68561.9 | 29.1 | 687918 | 687918 | 0 | ingress=2 |

</div>
<!-- perf-study-evidence:end -->

<!-- perf-study-deltas:start -->
## At a glance

- **collect vs emit (forward_fast):** `metrics_collect_only` costs about **5%** QPS versus `metrics_no_collect` (~70k vs ~73k); `metrics_collect_emit` costs about **6%** QPS versus `metrics_no_collect` (~69k vs ~73k).
<!-- perf-study-deltas:end -->

## Takeaway

**Collect carries most of the standing metrics cost; emit adds a thin band on
this median.** Versus no-collect (~73k), collect-only costs about **5%** (~70k);
collect+emit about **6%** (~69k).

**What to do:** turn off collect for categories you do not need. Choose minimal
vs standard with [operator metrics bases](/guides/operator-metrics-bases.md);
see the [metrics scrape tax](/performance/studies/metrics-scrape-ladder.md)
for export-facing scrape cost.

## Related guides

- [Metrics configurability](/observability/metrics-configurability.md)
- [Operator metrics bases](/guides/operator-metrics-bases.md)
- [Metrics scrape tax](/performance/studies/metrics-scrape-ladder.md)

## Member scenarios

- [feature-tax-metrics-no-collect-forward-fast](/performance/scenarios.md#feature-tax-metrics-no-collect-forward-fast)
- [feature-tax-metrics-collect-only-forward-fast](/performance/scenarios.md#feature-tax-metrics-collect-only-forward-fast)
- [feature-tax-metrics-collect-emit-forward-fast](/performance/scenarios.md#feature-tax-metrics-collect-emit-forward-fast)

## Related

- [Studies hub](/performance/studies/index.md)
- [Reference results](/performance/reference.md)
- [Methodology](/performance/methodology.md)
