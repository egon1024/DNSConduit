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
_Generated 2026-08-03T21:31:22Z from committed reference JSON (no live load suite in docs CI)._

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
| Run generated_at | `2026-08-03T21:17:39Z` |

Underlying JSON: [`perf/results/references/`](https://github.com/egon1024/DNSConduit/tree/main/perf/results/references) (see `latest-reference.json` pointer in a checkout).

Decision context: [Performance findings](/performance/index.md#findings) · [Tuning evidence (studies)](/performance/studies/index.md). Row intents: [Performance scenarios](/performance/scenarios.md) (glossary, not a browse-first surface).

## Scale

<div class="perf-chart" markdown="1">

### Achieved QPS — sync vs split_io (forward_fast)

![Achieved QPS — sync vs split_io (forward_fast)](generated/scale-sync-vs-split-io-forward-fast.svg)

[Download CSV](generated/scale-sync-vs-split-io-forward-fast.csv)

| Scenario | Runtime | Achieved QPS | Avg latency (ms) | Sent | Completed | Lost | Workers |
| --- | --- | --- | --- | --- | --- | --- | --- |
| [scale-sync-forward-fast](/performance/scenarios.md#scale-sync-forward-fast) | sync | 73594.4 | 27.1 | 738415 | 738415 | 0 | ingress=2 |
| [scale-split-io-forward-fast](/performance/scenarios.md#scale-split-io-forward-fast) | split_io | 141401.1 | 14.0 | 1430029 | 1430029 | 0 | ingress=2, policy=2, io=2 |

</div>

<div class="perf-chart" markdown="1">

### Achieved QPS — sync vs split_io (forward_slow)

![Achieved QPS — sync vs split_io (forward_slow)](generated/scale-sync-vs-split-io-forward-slow.svg)

[Download CSV](generated/scale-sync-vs-split-io-forward-slow.csv)

| Scenario | Runtime | Achieved QPS | Avg latency (ms) | Sent | Completed | Lost | Workers |
| --- | --- | --- | --- | --- | --- | --- | --- |
| [scale-sync-forward-slow](/performance/scenarios.md#scale-sync-forward-slow) | sync | 5.7 | 2509.2 | 12188 | 198 | 11990 | ingress=2 |
| [scale-split-io-forward-slow](/performance/scenarios.md#scale-split-io-forward-slow) | split_io | 38736.3 | 51.3 | 1167745 | 1167745 | 0 | ingress=2, policy=2, io=2 |

</div>

<div class="perf-chart" markdown="1">

### Achieved QPS — sync cache_hit

![Achieved QPS — sync cache_hit](generated/scale-cache-hit.svg)

[Download CSV](generated/scale-cache-hit.csv)

| Scenario | Runtime | Achieved QPS | Avg latency (ms) | Sent | Completed | Lost | Workers |
| --- | --- | --- | --- | --- | --- | --- | --- |
| [scale-sync-cache-hit](/performance/scenarios.md#scale-sync-cache-hit) | sync | 254403.3 | 7.8 | 2546522 | 2546522 | 0 | ingress=2 |

</div>

<div class="perf-chart" markdown="1">

### Achieved QPS — split_io topology (forward_fast)

![Achieved QPS — split_io topology (forward_fast)](generated/scale-topology-heavy.svg)

[Download CSV](generated/scale-topology-heavy.csv)

| Scenario | Runtime | Achieved QPS | Avg latency (ms) | Sent | Completed | Lost | Workers |
| --- | --- | --- | --- | --- | --- | --- | --- |
| [scale-split-io-forward-fast](/performance/scenarios.md#scale-split-io-forward-fast) | split_io | 141401.1 | 14.0 | 1430029 | 1430029 | 0 | ingress=2, policy=2, io=2 |
| [scale-split-io-topology-heavy](/performance/scenarios.md#scale-split-io-topology-heavy) | split_io | 210005.8 | 9.5 | 2102918 | 2102918 | 0 | ingress=4, policy=4, io=4 |

</div>

## Shutdown drain

<div class="perf-chart" markdown="1">

### Drain duration under forward_slow

![Drain duration under forward_slow](generated/shutdown-drain-forward-slow.svg)

[Download CSV](generated/shutdown-drain-forward-slow.csv)

| Drain policy | Drain duration (ms) | Client failures during stop | QPS | Avg latency (ms) | Sent | Completed |
| --- | --- | --- | --- | --- | --- | --- |
| [drain_complete](/performance/scenarios.md#shutdown-drain-complete-forward-slow) | 113.8 | 298 | 6.6 | 1008.6 | 377 | 79 |
| [drain_budgeted](/performance/scenarios.md#shutdown-drain-budgeted-forward-slow) | 163.6 | 200 | 10.2 | 1020.7 | 282 | 82 |
| [drain_minimal](/performance/scenarios.md#shutdown-drain-minimal-forward-slow) | 63.4 | 200 | 9.7 | 900.3 | 278 | 78 |

</div>

## Feature tax

<div class="perf-chart" markdown="1">

### Feature tax — metrics scrape tax (forward_fast)

![Feature tax — metrics scrape tax (forward_fast)](generated/feature-tax-metrics-scrape.svg)

[Download CSV](generated/feature-tax-metrics-scrape.csv)

| Posture | Achieved QPS | Avg latency (ms) | Sent | Completed | Lost |
| --- | --- | --- | --- | --- | --- |
| [metrics_off](/performance/scenarios.md#feature-tax-metrics-off-scrape-ladder-forward-fast) | 74751.2 | 26.7 | 749541 | 749541 | 0 |
| [minimal_scrape](/performance/scenarios.md#feature-tax-metrics-minimal-scrape-ladder-forward-fast) | 72057.1 | 27.7 | 723470 | 723470 | 0 |
| [standard_scrape](/performance/scenarios.md#feature-tax-metrics-standard-scrape-ladder-forward-fast) | 68348.4 | 29.2 | 686051 | 686051 | 0 |

</div>

<div class="perf-chart" markdown="1">

### Feature tax — dnstap off / sampled / full (forward_fast)

![Feature tax — dnstap off / sampled / full (forward_fast)](generated/feature-tax-dnstap.svg)

[Download CSV](generated/feature-tax-dnstap.csv)

| Posture | Achieved QPS | Avg latency (ms) | Sent | Completed | Lost |
| --- | --- | --- | --- | --- | --- |
| [dnstap_off](/performance/scenarios.md#feature-tax-dnstap-off-forward-fast) | 75505.8 | 26.4 | 757062 | 757062 | 0 |
| [dnstap_sampled](/performance/scenarios.md#feature-tax-dnstap-sampled-forward-fast) | 71108.5 | 28.0 | 714045 | 714045 | 0 |
| [dnstap_full](/performance/scenarios.md#feature-tax-dnstap-full-forward-fast) | 67911.4 | 29.4 | 681855 | 681855 | 0 |

</div>

<div class="perf-chart" markdown="1">

### Feature tax — collect vs emit (forward_fast)

![Feature tax — collect vs emit (forward_fast)](generated/feature-tax-collect-emit.svg)

[Download CSV](generated/feature-tax-collect-emit.csv)

| Posture | Achieved QPS | Avg latency (ms) | Sent | Completed | Lost |
| --- | --- | --- | --- | --- | --- |
| [no_collect](/performance/scenarios.md#feature-tax-metrics-no-collect-forward-fast) | 72940.1 | 27.3 | 732015 | 732015 | 0 |
| [collect_only](/performance/scenarios.md#feature-tax-metrics-collect-only-forward-fast) | 69562.3 | 28.7 | 697649 | 697649 | 0 |
| [collect_emit](/performance/scenarios.md#feature-tax-metrics-collect-emit-forward-fast) | 68561.9 | 29.1 | 687918 | 687918 | 0 |

</div>

<div class="perf-chart" markdown="1">

### Feature tax — metrics and dnstap combined (forward_fast)

![Feature tax — metrics and dnstap combined (forward_fast)](generated/feature-tax-combined.svg)

[Download CSV](generated/feature-tax-combined.csv)

| Posture | Achieved QPS | Avg latency (ms) | Sent | Completed | Lost |
| --- | --- | --- | --- | --- | --- |
| [metrics_off](/performance/scenarios.md#feature-tax-metrics-off-forward-fast) | 75250.1 | 26.5 | 754909 | 754909 | 0 |
| [standard_scrape](/performance/scenarios.md#feature-tax-metrics-standard-scrape-forward-fast) | 69858.0 | 28.6 | 700756 | 700756 | 0 |
| [dnstap_full](/performance/scenarios.md#feature-tax-dnstap-full-forward-fast) | 67911.4 | 29.4 | 681855 | 681855 | 0 |
| [standard_dnstap_full](/performance/scenarios.md#feature-tax-metrics-standard-dnstap-full-forward-fast) | 63930.1 | 31.2 | 641638 | 641638 | 0 |

</div>

<div class="perf-chart" markdown="1">

### Feature tax — scrape hammer under load (forward_fast)

![Feature tax — scrape hammer under load (forward_fast)](generated/feature-tax-scrape-hammer.svg)

[Download CSV](generated/feature-tax-scrape-hammer.csv)

| Posture | Achieved QPS | Avg latency (ms) | Sent | Completed | Lost |
| --- | --- | --- | --- | --- | --- |
| [metrics_off](/performance/scenarios.md#feature-tax-metrics-off-forward-fast) | 75250.1 | 26.5 | 754909 | 754909 | 0 |
| [standard_scrape](/performance/scenarios.md#feature-tax-metrics-standard-scrape-forward-fast) | 69858.0 | 28.6 | 700756 | 700756 | 0 |
| [scrape_hammer](/performance/scenarios.md#feature-tax-metrics-standard-scrape-hammer-forward-fast) | 60779.5 | 32.8 | 611124 | 611124 | 0 |

</div>

<div class="perf-chart" markdown="1">

### Feature tax — metrics scrape under split_io (forward_fast)

![Feature tax — metrics scrape under split_io (forward_fast)](generated/feature-tax-scrape-split-io.svg)

[Download CSV](generated/feature-tax-scrape-split-io.csv)

| Posture | Achieved QPS | Avg latency (ms) | Sent | Completed | Lost |
| --- | --- | --- | --- | --- | --- |
| [metrics_off](/performance/scenarios.md#feature-tax-metrics-off-split-io-forward-fast) | 139604.8 | 14.3 | 1397888 | 1397888 | 0 |
| [standard_scrape](/performance/scenarios.md#feature-tax-metrics-standard-scrape-split-io-forward-fast) | 142733.8 | 14.0 | 1429317 | 1429317 | 0 |

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
