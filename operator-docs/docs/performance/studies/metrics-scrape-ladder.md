# Metrics scrape tax

<div class="study-question" markdown="1">

What does enabling richer Prometheus scrape metrics cost under [`forward_fast`](/performance/methodology.md#load-shapes)?

</div>

Numbers are same-host comparisons on a single reference host and are **not**
service-level objectives. See the
[performance hub disclaimer](/performance/index.md).

## When this matters

Operators choose [`metrics.base`](/observability/metrics-configurability.md)
(`minimal` vs `standard`) for dashboard richness versus cardinality and hot-path
cost. See [Operator metrics bases](/guides/operator-metrics-bases.md). This study
puts an observability-off baseline next to minimal and standard scrape postures
under the same [`forward_fast`](/performance/methodology.md#load-shapes) load. For OTLP push cost under the same recipe, see
[OTLP tax under load](/performance/studies/otlp-tax-under-load.md).

## What we varied

- **Varied:** observability posture
  ([`metrics_off`](/performance/scenarios.md#feature-tax-metrics-off-scrape-ladder-forward-fast) →
  [`metrics_minimal_scrape`](/performance/scenarios.md#feature-tax-metrics-minimal-scrape-ladder-forward-fast) →
  [`metrics_standard_scrape`](/performance/scenarios.md#feature-tax-metrics-standard-scrape-ladder-forward-fast))
- **Held constant:** [`sync`](/concepts/runtime-and-concurrency.md#sync-runtime-default) runtime,
  [`forward_fast`](/performance/methodology.md#load-shapes) stub, ingress workers
- **Loadgen:** published [`forward_fast`](/performance/methodology.md#load-shapes)
  dnsperf recipe — `-c 16` / `-T 8` / `-q 2000` (shared with other
  `forward_fast` / `cache_hit` scale and feature-tax cells; see
  [How load is applied](/performance/methodology.md#how-load-is-applied-not-a-fixed-offered-qps))
  — each pole is also the **median of 3 independent rounds**

<!-- perf-ann:ann-feature-tax-scrape-ladder-noise:start -->
!!! note "Metrics scrape tax — published forward_fast recipe"
    These scrape-tax cells use the same published [`forward_fast`](/performance/methodology.md#load-shapes)
    dnsperf recipe as other scale / feature-tax fast cells (clients 16, threads 8,
    max outstanding 2000) and are the **median of 3 independent rounds**. That is
    the shared publish recipe so achieved QPS reflects Conduit capacity rather than
    a thin outstanding window — not a study-only workaround. See
    [How load is applied](/performance/methodology.md#how-load-is-applied-not-a-fixed-offered-qps).
    Remeasure locally with the same recipe before sizing.
<!-- perf-ann:ann-feature-tax-scrape-ladder-noise:end -->

<!-- perf-study-evidence:start -->
## Evidence

<div class="perf-chart" markdown="1">

### Feature tax — metrics scrape tax (forward_fast)

![Feature tax — metrics scrape tax (forward_fast)](../generated/metrics-scrape-ladder-forward-fast.svg)

[Download CSV](../generated/metrics-scrape-ladder-forward-fast.csv)

| Posture | Runtime | Achieved QPS | Avg latency (ms) | Sent | Completed | Lost | Workers |
| --- | --- | --- | --- | --- | --- | --- | --- |
| [metrics_off](/performance/scenarios.md#feature-tax-metrics-off-scrape-ladder-forward-fast) | sync | 75321.6 | 3.9 | 757077 | 753665 | 3410 | ingress=2 |
| [metrics_minimal_scrape](/performance/scenarios.md#feature-tax-metrics-minimal-scrape-ladder-forward-fast) | sync | 73415.2 | 4.3 | 737853 | 734479 | 3393 | ingress=2 |
| [metrics_standard_scrape](/performance/scenarios.md#feature-tax-metrics-standard-scrape-ladder-forward-fast) | sync | 66697.3 | 2.6 | 670934 | 667270 | 3664 | ingress=2 |

</div>
<!-- perf-study-evidence:end -->

<!-- perf-study-deltas:start -->
## At a glance

- **metrics scrape tax (forward_fast):** `metrics_minimal_scrape` costs about **3%** QPS versus `metrics_off` (~73k vs ~75k); `metrics_standard_scrape` costs about **11%** QPS versus `metrics_off` (~67k vs ~75k).
<!-- perf-study-deltas:end -->

## Takeaway

**Richer Prometheus scrape costs more QPS.** Versus observability off on this
lab, minimal scrape costs about **3%** and standard scrape about **11%** (~75k /
~73k / ~67k). That is a same-host tax signal, not an SLO.

**What to do:** pick minimal vs standard from the cardinality you need
([operator metrics bases](/guides/operator-metrics-bases.md)), then remeasure on
your hardware (`--study metrics-scrape-ladder`). See also
[Metrics collect vs emit](/performance/studies/metrics-collect-vs-emit.md) and,
for [`split_io`](/concepts/runtime-and-concurrency.md#split-io-runtime),
[Metrics scrape (split_io)](/performance/studies/metrics-scrape-split-io.md).

## Related guides

- [Operator metrics bases](/guides/operator-metrics-bases.md)
- [Metrics configurability](/observability/metrics-configurability.md)
- [Reference: metrics and tracing](/reference/config-schema/metrics-and-tracing.md)
- [How load is applied](/performance/methodology.md#how-load-is-applied-not-a-fixed-offered-qps)
- [OTLP tax under load](/performance/studies/otlp-tax-under-load.md)
- [Metrics collect vs emit](/performance/studies/metrics-collect-vs-emit.md)
- [Aggressive scrape cadence](/performance/studies/metrics-scrape-hammer.md)

## Member scenarios

- [feature-tax-metrics-off-scrape-ladder-forward-fast](/performance/scenarios.md#feature-tax-metrics-off-scrape-ladder-forward-fast)
- [feature-tax-metrics-minimal-scrape-ladder-forward-fast](/performance/scenarios.md#feature-tax-metrics-minimal-scrape-ladder-forward-fast)
- [feature-tax-metrics-standard-scrape-ladder-forward-fast](/performance/scenarios.md#feature-tax-metrics-standard-scrape-ladder-forward-fast)

## Related

- [Studies hub](/performance/studies/index.md)
- [Reference results](/performance/reference.md)
- [Methodology](/performance/methodology.md)
