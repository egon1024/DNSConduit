# Aggressive scrape cadence under load

What does frequent Prometheus scraping during load cost versus listener-only scrape?

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
| [metrics_off](/performance/scenarios.md#feature-tax-metrics-off-forward-fast) | sync | 138760.8 | 2.0 | 1391423 | 1387890 | 3449 | ingress=2 |
| [metrics_standard_scrape](/performance/scenarios.md#feature-tax-metrics-standard-scrape-forward-fast) | sync | 121399.7 | 2.3 | 1217696 | 1214291 | 3449 | ingress=2 |
| [metrics_standard_scrape_hammer](/performance/scenarios.md#feature-tax-metrics-standard-scrape-hammer-forward-fast) | sync | 117989.8 | 2.4 | 1183610 | 1180188 | 3441 | ingress=2 |

</div>
<!-- perf-study-evidence:end -->

## Takeaway

On this published reference, **listener-only standard scrape costs about 12%**
versus observability off, and **aggressive external scraping (~10/s) adds
another 3 points** on top of that (~15% total dip versus off; lab absolute
~139k / ~121k / ~118k) — the direction you would expect: more scrape traffic
costs a bit more. **Operator posture:** size the bulk of scrape tax from the
[metrics scrape ladder](/performance/studies/metrics-scrape-ladder.md) (and
[collect vs emit](/performance/studies/metrics-collect-vs-emit.md)); the
incremental cost of a busier scraper on top of that is modest but not free —
set your real scrape interval from ops needs. The hammer cell still shows the
harness exercises the scrape path under load (secondary `scrape_hammer_ok`
counts in run JSON).

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
