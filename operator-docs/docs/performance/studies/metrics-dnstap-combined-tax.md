# Combined metrics + dnstap tax

What does turning on standard scrape and fuller dnstap together cost?

Numbers are same-host comparisons (relative to baselines measured on one named
lab profile) and are **not** service-level objectives. See the
[performance hub disclaimer](/performance/index.md).

## When this matters

Production postures often enable **more than one** observability surface.
Studies that turn on only metrics scrape or only dnstap
([metrics scrape](/performance/studies/metrics-scrape-ladder.md),
[dnstap emit](/performance/studies/dnstap-emit-tax.md)) miss interaction cost.
This study places observability off, standard scrape alone, fuller dnstap alone,
and **both** under the same [`forward_fast`](/performance/methodology.md#load-shapes) recipe.

## What we varied

- **Varied:** observability posture
  ([`metrics_off`](/performance/scenarios.md#feature-tax-metrics-off-forward-fast),
  [`metrics_standard_scrape`](/performance/scenarios.md#feature-tax-metrics-standard-scrape-forward-fast),
  [`dnstap_full`](/performance/scenarios.md#feature-tax-dnstap-full-forward-fast),
  [`metrics_standard_dnstap_full`](/performance/scenarios.md#feature-tax-metrics-standard-dnstap-full-forward-fast))
- **Held fixed:** [`sync`](/concepts/runtime-and-concurrency.md#sync-runtime-default) runtime,
  [`forward_fast`](/performance/methodology.md#load-shapes), ingress workers, dnsperf recipe

<!-- perf-study-evidence:start -->
## Evidence

<div class="perf-chart" markdown="1">

### Feature tax — metrics and dnstap combined (forward_fast)

![Feature tax — metrics and dnstap combined (forward_fast)](../generated/metrics-dnstap-combined-forward-fast.svg)

[Download CSV](../generated/metrics-dnstap-combined-forward-fast.csv)

| Posture | Runtime | Achieved QPS | Avg latency (ms) | Sent | Completed | Lost | Workers |
| --- | --- | --- | --- | --- | --- | --- | --- |
| [metrics_off](/performance/scenarios.md#feature-tax-metrics-off-forward-fast) | sync | 138760.8 | 2.0 | 1391423 | 1387890 | 3449 | ingress=2 |
| [metrics_standard_scrape](/performance/scenarios.md#feature-tax-metrics-standard-scrape-forward-fast) | sync | 121399.7 | 2.3 | 1217696 | 1214291 | 3449 | ingress=2 |
| [dnstap_full](/performance/scenarios.md#feature-tax-dnstap-full-forward-fast) | sync | 113838.8 | 2.6 | 1142096 | 1138671 | 3425 | ingress=2 |
| [metrics_standard_dnstap_full](/performance/scenarios.md#feature-tax-metrics-standard-dnstap-full-forward-fast) | sync | 106763.8 | 1.7 | 1071331 | 1067929 | 3553 | ingress=2 |

</div>
<!-- perf-study-evidence:end -->

## Takeaway

On this published reference, **standard scrape alone costs about 12%** achieved
QPS versus observability off, **fuller dnstap alone costs about 18%**, and the
**combined** posture costs about **23%** — roughly additive rather than
dominated by one surface (lab absolute ~139k / ~121k / ~114k / ~107k).
**Operator posture:** each surface adds its own tax on top of the other; turn
on each surface only if you need it; use the
[metrics scrape ladder](/performance/studies/metrics-scrape-ladder.md) and
[dnstap emit tax](/performance/studies/dnstap-emit-tax.md) for single-feature
sizing, then remeasure the pair on your hardware before production.

## Related guides

- [Metrics configurability](/observability/metrics-configurability.md)
- [Event export](/observability/event-export.md)
- [Operator metrics bases](/guides/operator-metrics-bases.md)
- [Dnstap emit tax](/performance/studies/dnstap-emit-tax.md)
- [Metrics scrape ladder](/performance/studies/metrics-scrape-ladder.md)

## Member scenarios

- [feature-tax-metrics-off-forward-fast](/performance/scenarios.md#feature-tax-metrics-off-forward-fast)
- [feature-tax-metrics-standard-scrape-forward-fast](/performance/scenarios.md#feature-tax-metrics-standard-scrape-forward-fast)
- [feature-tax-dnstap-full-forward-fast](/performance/scenarios.md#feature-tax-dnstap-full-forward-fast)
- [feature-tax-metrics-standard-dnstap-full-forward-fast](/performance/scenarios.md#feature-tax-metrics-standard-dnstap-full-forward-fast)

## Related

- [Studies hub](/performance/studies/index.md)
- [Reference results](/performance/reference.md)
- [Methodology](/performance/methodology.md)
