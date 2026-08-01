# Metrics collect vs emit

Is metrics cost dominated by hot-path collect, or by scrape emit?

Numbers are same-host comparisons (relative to baselines measured on one named
lab profile) and are **not** service-level objectives. See the
[performance hub disclaimer](/performance/index.md).

## When this matters

[`metrics.collection`](/observability/metrics-configurability.md) can record
hot-path series without exporting them (`collect: true`, `emit: false`), or skip
recording entirely. Operators often ask whether the tax is **recording** or
**scrape export**. This study keeps `metrics.base: standard` and varies
collect/emit on the volume/failures/lookup/timing categories under [`forward_fast`](/performance/methodology.md#load-shapes).
See also the [metrics scrape ladder](/performance/studies/metrics-scrape-ladder.md)
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
| [metrics_no_collect](/performance/scenarios.md#feature-tax-metrics-no-collect-forward-fast) | sync | 128663.3 | 2.1 | 1290379 | 1286917 | 3462 | ingress=2 |
| [metrics_collect_only](/performance/scenarios.md#feature-tax-metrics-collect-only-forward-fast) | sync | 121086.6 | 1.6 | 1214611 | 1211152 | 3484 | ingress=2 |
| [metrics_collect_emit](/performance/scenarios.md#feature-tax-metrics-collect-emit-forward-fast) | sync | 117515.8 | 2.6 | 1178865 | 1175436 | 3429 | ingress=2 |

</div>
<!-- perf-study-evidence:end -->

## Takeaway

On this published reference, **collect-only costs about 6%** achieved QPS
versus **no-collect**, and **collect+emit costs about 9%** — hot-path recording
shows up, and scrape export adds a smaller additional tax on top of it (lab
absolute ~129k / ~121k / ~118k). **Operator posture:** turn off collect for
categories you do not need — most of the cost is recording, not exporting, but
exporting is not free either. Prefer
[operator metrics bases](/guides/operator-metrics-bases.md) for minimal vs
standard, and the
[metrics scrape ladder](/performance/studies/metrics-scrape-ladder.md) for
export-facing scrape cost.

## Related guides

- [Metrics configurability](/observability/metrics-configurability.md)
- [Operator metrics bases](/guides/operator-metrics-bases.md)
- [Metrics scrape ladder](/performance/studies/metrics-scrape-ladder.md)

## Member scenarios

- [feature-tax-metrics-no-collect-forward-fast](/performance/scenarios.md#feature-tax-metrics-no-collect-forward-fast)
- [feature-tax-metrics-collect-only-forward-fast](/performance/scenarios.md#feature-tax-metrics-collect-only-forward-fast)
- [feature-tax-metrics-collect-emit-forward-fast](/performance/scenarios.md#feature-tax-metrics-collect-emit-forward-fast)

## Related

- [Studies hub](/performance/studies/index.md)
- [Reference results](/performance/reference.md)
- [Methodology](/performance/methodology.md)
