---
toc_depth: 3
toc_collapsible: true
---

# Performance reference results

Dense chart and CSV warehouse for a single reference host (`maintainer-ws-1`).
For tuning decisions, start from
[Performance findings](/performance/index.md#findings) and
[Tuning evidence (studies)](/performance/studies/index.md); use this page when you
need every published figure in one place. Prefer reading each chart relative to
its baseline cells on that host. These are **not** service-level objectives.
Reproduce on your hardware with the
[harness instructions](/performance/reproduce.md) before making capacity decisions.
Absolute QPS is not a portable cross-host capacity claim.

<!-- perf-ann:ann-forward-slow-lossy-context:start -->
!!! note "Reading forward_slow cells — saturation against a 50 ms backend"
    [`forward_slow`](/performance/methodology.md#load-shapes) cells offer far more
    concurrency than a runtime that blocks on upstream latency can absorb, which is
    the point: they show what happens to each runtime model when the backend is slow
    and the client keeps asking. Read both columns together. A runtime that
    multiplexes in-flight queries answers near the upstream delay itself; a runtime
    that occupies a worker for the whole round trip reports a small fraction of that
    throughput and an average latency of seconds, because queries wait in Conduit
    rather than at the backend. Every published cell here is measured on a stub
    upstream that stays well clear of saturation and is checked for successful
    answers, so the numbers are Conduit's behavior, not the harness reaching its
    limit. See
    [How load is applied](/performance/methodology.md#how-load-is-applied-not-a-fixed-offered-qps)
    and [Only successful answers count](/performance/methodology.md#only-successful-answers-count).
<!-- perf-ann:ann-forward-slow-lossy-context:end -->

<!-- perf-reference-body:start -->
_Generated 2026-08-02T21:04:57Z from committed reference JSON (no live load suite in docs CI)._

## Lab profile

| Field | Value |
| --- | --- |
| Profile id | `maintainer-ws-1` |
| Display name | Maintainer workstation (maintainer-ws-1) |
| CPU | Intel(R) Core(TM) i9-14900K |
| Cores (physical / logical) | 32 / 32 |
| OS | Linux 6.8.0-136-generic |
| Conduit | `target/release/conduit` (unknown) |
| Loadgen | dnsperf mode=`docker` |
| Run generated_at | `2026-08-02T17:48:43Z` |

Underlying JSON: [`perf/results/references/`](https://github.com/egon1024/DNSConduit/tree/main/perf/results/references) (see `latest-reference.json` pointer in a checkout).

Decision context: [Performance findings](/performance/index.md#findings) · [Tuning evidence (studies)](/performance/studies/index.md). Row intents: [Performance scenarios](/performance/scenarios.md) (glossary, not a browse-first surface).

## Scale

<div class="perf-chart" markdown="1">

### Achieved QPS — sync vs split_io (forward_fast)

![Achieved QPS — sync vs split_io (forward_fast)](generated/scale-sync-vs-split-io-forward-fast.svg)

[Download CSV](generated/scale-sync-vs-split-io-forward-fast.csv)

| Scenario | Runtime | Achieved QPS | Avg latency (ms) | Sent | Completed | Lost | Workers |
| --- | --- | --- | --- | --- | --- | --- | --- |
| [scale-sync-forward-fast](/performance/scenarios.md#scale-sync-forward-fast) | sync | 75601.1 | 3.8 | 759747 | 756344 | 3443 | ingress=2 |
| [scale-split-io-forward-fast](/performance/scenarios.md#scale-split-io-forward-fast) | split_io | 147142.3 | 1.2 | 1479133 | 1475210 | 3912 | ingress=2, policy=2, io=2 |

</div>

<div class="perf-chart" markdown="1">

### Achieved QPS — sync vs split_io (forward_slow)

![Achieved QPS — sync vs split_io (forward_slow)](generated/scale-sync-vs-split-io-forward-slow.svg)

[Download CSV](generated/scale-sync-vs-split-io-forward-slow.csv)

| Scenario | Runtime | Achieved QPS | Avg latency (ms) | Sent | Completed | Lost | Workers |
| --- | --- | --- | --- | --- | --- | --- | --- |
| [scale-sync-forward-slow](/performance/scenarios.md#scale-sync-forward-slow) | sync | 5.7 | 2507.6 | 12188 | 198 | 11990 | ingress=2 |
| — | — | — | — | — | — | — | — |

</div>

<div class="perf-chart" markdown="1">

### Achieved QPS — sync cache_hit

![Achieved QPS — sync cache_hit](generated/scale-cache-hit.svg)

[Download CSV](generated/scale-cache-hit.csv)

| Scenario | Runtime | Achieved QPS | Avg latency (ms) | Sent | Completed | Lost | Workers |
| --- | --- | --- | --- | --- | --- | --- | --- |
| [scale-sync-cache-hit](/performance/scenarios.md#scale-sync-cache-hit) | sync | 333042.1 | 1.0 | 3334080 | 3330738 | 3398 | ingress=2 |

</div>

<div class="perf-chart" markdown="1">

### Achieved QPS — split_io topology (forward_fast)

![Achieved QPS — split_io topology (forward_fast)](generated/scale-topology-heavy.svg)

[Download CSV](generated/scale-topology-heavy.csv)

| Scenario | Runtime | Achieved QPS | Avg latency (ms) | Sent | Completed | Lost | Workers |
| --- | --- | --- | --- | --- | --- | --- | --- |
| [scale-split-io-forward-fast](/performance/scenarios.md#scale-split-io-forward-fast) | split_io | 147142.3 | 1.2 | 1479133 | 1475210 | 3912 | ingress=2, policy=2, io=2 |
| — | — | — | — | — | — | — | — |

</div>

## Shutdown drain

<div class="perf-chart" markdown="1">

### Drain duration under forward_slow

![Drain duration under forward_slow](generated/shutdown-drain-forward-slow.svg)

[Download CSV](generated/shutdown-drain-forward-slow.csv)

| Drain policy | Drain duration (ms) | Client failures during stop | QPS | Avg latency (ms) | Sent | Completed |
| --- | --- | --- | --- | --- | --- | --- |
| [drain_complete](/performance/scenarios.md#shutdown-drain-complete-forward-slow) | 113.6 | 296 | 6.7 | 976.6 | 376 | 80 |
| [drain_budgeted](/performance/scenarios.md#shutdown-drain-budgeted-forward-slow) | 63.4 | 200 | 9.7 | 896.6 | 278 | 78 |
| [drain_minimal](/performance/scenarios.md#shutdown-drain-minimal-forward-slow) | 63.4 | 200 | 9.7 | 995.7 | 278 | 78 |

</div>

## Feature tax

<div class="perf-chart" markdown="1">

### Feature tax — metrics scrape tax (forward_fast)

![Feature tax — metrics scrape tax (forward_fast)](generated/feature-tax-metrics-scrape.svg)

[Download CSV](generated/feature-tax-metrics-scrape.csv)

| Posture | Achieved QPS | Avg latency (ms) | Sent | Completed | Lost |
| --- | --- | --- | --- | --- | --- |
| [metrics_off](/performance/scenarios.md#feature-tax-metrics-off-scrape-ladder-forward-fast) | 69443.0 | 2.5 | 698389 | 694725 | 3664 |
| [minimal_scrape](/performance/scenarios.md#feature-tax-metrics-minimal-scrape-ladder-forward-fast) | 71784.8 | 4.3 | 721553 | 718176 | 3401 |
| [standard_scrape](/performance/scenarios.md#feature-tax-metrics-standard-scrape-ladder-forward-fast) | 66823.7 | 2.9 | 672247 | 668583 | 3664 |

</div>

<div class="perf-chart" markdown="1">

### Feature tax — dnstap off / sampled / full (forward_fast)

![Feature tax — dnstap off / sampled / full (forward_fast)](generated/feature-tax-dnstap.svg)

[Download CSV](generated/feature-tax-dnstap.csv)

| Posture | Achieved QPS | Avg latency (ms) | Sent | Completed | Lost |
| --- | --- | --- | --- | --- | --- |
| [dnstap_off](/performance/scenarios.md#feature-tax-dnstap-off-forward-fast) | 75250.1 | 4.0 | 756222 | 752823 | 3399 |
| [dnstap_sampled](/performance/scenarios.md#feature-tax-dnstap-sampled-forward-fast) | 72484.3 | 4.3 | 728574 | 725189 | 3385 |
| [dnstap_full](/performance/scenarios.md#feature-tax-dnstap-full-forward-fast) | 67178.8 | 4.4 | 675731 | 672299 | 3432 |

</div>

<div class="perf-chart" markdown="1">

### Feature tax — collect vs emit (forward_fast)

![Feature tax — collect vs emit (forward_fast)](generated/feature-tax-collect-emit.svg)

[Download CSV](generated/feature-tax-collect-emit.csv)

| Posture | Achieved QPS | Avg latency (ms) | Sent | Completed | Lost |
| --- | --- | --- | --- | --- | --- |
| [no_collect](/performance/scenarios.md#feature-tax-metrics-no-collect-forward-fast) | 67772.4 | 2.7 | 681748 | 678084 | 3664 |
| [collect_only](/performance/scenarios.md#feature-tax-metrics-collect-only-forward-fast) | 69703.6 | 4.2 | 700769 | 697334 | 3418 |
| [collect_emit](/performance/scenarios.md#feature-tax-metrics-collect-emit-forward-fast) | 63894.6 | 2.8 | 642896 | 639232 | 3664 |

</div>

<div class="perf-chart" markdown="1">

### Feature tax — metrics and dnstap combined (forward_fast)

![Feature tax — metrics and dnstap combined (forward_fast)](generated/feature-tax-combined.svg)

[Download CSV](generated/feature-tax-combined.csv)

| Posture | Achieved QPS | Avg latency (ms) | Sent | Completed | Lost |
| --- | --- | --- | --- | --- | --- |
| [metrics_off](/performance/scenarios.md#feature-tax-metrics-off-forward-fast) | 75462.4 | 4.0 | 758529 | 755155 | 3400 |
| [standard_scrape](/performance/scenarios.md#feature-tax-metrics-standard-scrape-forward-fast) | 69628.0 | 4.5 | 700042 | 696579 | 3377 |
| [dnstap_full](/performance/scenarios.md#feature-tax-dnstap-full-forward-fast) | 67178.8 | 4.4 | 675731 | 672299 | 3432 |
| [standard_dnstap_full](/performance/scenarios.md#feature-tax-metrics-standard-dnstap-full-forward-fast) | 63336.3 | 4.5 | 637160 | 633740 | 3430 |

</div>

<div class="perf-chart" markdown="1">

### Feature tax — scrape hammer under load (forward_fast)

![Feature tax — scrape hammer under load (forward_fast)](generated/feature-tax-scrape-hammer.svg)

[Download CSV](generated/feature-tax-scrape-hammer.csv)

| Posture | Achieved QPS | Avg latency (ms) | Sent | Completed | Lost |
| --- | --- | --- | --- | --- | --- |
| [metrics_off](/performance/scenarios.md#feature-tax-metrics-off-forward-fast) | 75462.4 | 4.0 | 758529 | 755155 | 3400 |
| [standard_scrape](/performance/scenarios.md#feature-tax-metrics-standard-scrape-forward-fast) | 69628.0 | 4.5 | 700042 | 696579 | 3377 |
| [scrape_hammer](/performance/scenarios.md#feature-tax-metrics-standard-scrape-hammer-forward-fast) | 68340.8 | 4.5 | 687351 | 683962 | 3389 |

</div>

<div class="perf-chart" markdown="1">

### Feature tax — metrics scrape under split_io (forward_fast)

![Feature tax — metrics scrape under split_io (forward_fast)](generated/feature-tax-scrape-split-io.svg)

[Download CSV](generated/feature-tax-scrape-split-io.csv)

| Posture | Achieved QPS | Avg latency (ms) | Sent | Completed | Lost |
| --- | --- | --- | --- | --- | --- |
| [metrics_off](/performance/scenarios.md#feature-tax-metrics-off-split-io-forward-fast) | 130924.8 | 1.4 | 1318112 | 1314408 | 3870 |
| [standard_scrape](/performance/scenarios.md#feature-tax-metrics-standard-scrape-split-io-forward-fast) | 134804.5 | 1.8 | 1368825 | 1365195 | 3702 |

</div>
<!-- perf-reference-body:end -->

## Related

- [Performance findings](/performance/index.md#findings)
- [Tuning evidence (studies)](/performance/studies/index.md)
- [Methodology](/performance/methodology.md)
- [Reproduce against a binary](/performance/reproduce.md)
- [Scenario descriptions](/performance/scenarios.md) (row glossary)
- [Runtime and concurrency](/concepts/runtime-and-concurrency.md)
- [Dataplane runtime tuning](/guides/dataplane-runtime-tuning.md)
