# Aggressive scrape cadence under load

<div class="study-question" markdown="1">

What does frequent Prometheus scraping during load cost versus listener-only scrape?

</div>

Numbers are same-host comparisons (relative to baselines measured on one named
lab profile) and are **not** service-level objectives. See the
[performance hub disclaimer](/performance/index.md).

## When this matters

Opening a Prometheus listen address without concurrent scrapes underestimates
export-path cost. This study adds a lab **scrape hammer** (HTTP GET to
`/metrics` about every 100 ms) while dnsperf runs, compared to observability off
and standard scrape with the listener idle. Use it when your Prometheus (or other
scraper) hits Conduit aggressively under peak traffic.

## What we varied

- **Varied:** scrape cadence posture
  ([`metrics_off`](/performance/scenarios.md#feature-tax-metrics-off-forward-fast),
  [`metrics_standard_scrape`](/performance/scenarios.md#feature-tax-metrics-standard-scrape-forward-fast),
  [`metrics_standard_scrape_hammer`](/performance/scenarios.md#feature-tax-metrics-standard-scrape-hammer-forward-fast))
- **Held fixed:** [`sync`](/concepts/runtime-and-concurrency.md#sync-runtime-default), [`forward_fast`](/performance/methodology.md#load-shapes), standard base on the scrape configurations

<!-- perf-study-evidence:start -->
## Evidence

<div class="perf-chart" markdown="1">

### Feature tax — scrape hammer under load (forward_fast)

![Feature tax — scrape hammer under load (forward_fast)](../generated/metrics-scrape-hammer-forward-fast.svg)

[Download CSV](../generated/metrics-scrape-hammer-forward-fast.csv)

| Posture | Runtime | Achieved QPS | Avg latency (ms) | Sent | Completed | Lost | Workers |
| --- | --- | --- | --- | --- | --- | --- | --- |
| [metrics_off](/performance/scenarios.md#feature-tax-metrics-off-forward-fast) | sync | 77319.5 | 3.8 | 776925 | 773506 | 3419 | ingress=2 |
| [metrics_standard_scrape](/performance/scenarios.md#feature-tax-metrics-standard-scrape-forward-fast) | sync | 69173.3 | 2.6 | 695725 | 692061 | 3664 | ingress=2 |
| [metrics_standard_scrape_hammer](/performance/scenarios.md#feature-tax-metrics-standard-scrape-hammer-forward-fast) | sync | 70104.3 | 4.4 | 704809 | 701424 | 3385 | ingress=2 |

</div>
<!-- perf-study-evidence:end -->

<!-- perf-study-deltas:start -->
## At a glance

- **scrape hammer under load (forward_fast):** `metrics_standard_scrape` costs about **11%** QPS versus `metrics_off` (~69k vs ~77k); `metrics_standard_scrape_hammer` costs about **9%** QPS versus `metrics_off` (~70k vs ~77k).
<!-- perf-study-deltas:end -->

## Takeaway

**A busier scraper did not add a clear extra tax beyond standard scrape on this
lab.** Versus observability off, listener-only standard scrape costs about
**11%** QPS; aggressive external scrape (~10/s) lands in the same band (~9%;
~77k / ~69k / ~70k).

**What to do:** size the bulk of scrape cost from the
[metrics scrape ladder](/performance/studies/metrics-scrape-ladder.md) and
[collect vs emit](/performance/studies/metrics-collect-vs-emit.md). Choose your
scrape interval for ops needs; remeasure if your scraper is far hotter than this
lab hammer.

## Related guides

- [Metrics](/observability/metrics.md)
- [Operator metrics bases](/guides/operator-metrics-bases.md)
- [Metrics scrape ladder](/performance/studies/metrics-scrape-ladder.md)
- [Metrics scrape (split_io)](/performance/studies/metrics-scrape-split-io.md)

## Member scenarios

- [feature-tax-metrics-off-forward-fast](/performance/scenarios.md#feature-tax-metrics-off-forward-fast)
- [feature-tax-metrics-standard-scrape-forward-fast](/performance/scenarios.md#feature-tax-metrics-standard-scrape-forward-fast)
- [feature-tax-metrics-standard-scrape-hammer-forward-fast](/performance/scenarios.md#feature-tax-metrics-standard-scrape-hammer-forward-fast)

## Related

- [Studies hub](/performance/studies/index.md)
- [Reference results](/performance/reference.md)
- [Methodology](/performance/methodology.md)
