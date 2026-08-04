# Dnstap emit tax

<div class="study-question" markdown="1">

What does sampled vs fuller dnstap emission cost versus dnstap off?

</div>

Numbers are same-host comparisons on a single reference host and are **not**
service-level objectives. See the
[performance hub disclaimer](/performance/index.md).

## When this matters

[Event export](/observability/event-export.md) (dnstap / events) is powerful for
traffic forensics and can tax the datapath when emitting query/response
surfaces. Export is designed to stay off the DNS hot path — a full queue
[drops events](/observability/event-export.md#overload-and-metrics) rather than
delaying client replies — but producers still pay to build and enqueue frames.
Compare sinks off, sampled (~10% responses), and fuller emit under the same
[`forward_fast`](/performance/methodology.md#load-shapes) load before enabling broad production capture. Lab walkthrough:
[Event export and dnstap](/guides/event-export-dnstap.md).

## What we varied

- **Varied:** dnstap/events posture
  ([`dnstap_off`](/performance/scenarios.md#feature-tax-dnstap-off-forward-fast) →
  [`dnstap_sampled`](/performance/scenarios.md#feature-tax-dnstap-sampled-forward-fast) →
  [`dnstap_full`](/performance/scenarios.md#feature-tax-dnstap-full-forward-fast))
- **Held constant:** [`sync`](/concepts/runtime-and-concurrency.md#sync-runtime-default) runtime,
  [`forward_fast`](/performance/methodology.md#load-shapes), ingress workers, dnsperf recipe

<!-- perf-study-evidence:start -->
## Evidence

<div class="perf-chart" markdown="1">

### Feature tax — dnstap off / sampled / full (forward_fast)

![Feature tax — dnstap off / sampled / full (forward_fast)](../generated/dnstap-off-sampled-full-forward-fast.svg)

[Download CSV](../generated/dnstap-off-sampled-full-forward-fast.csv)

| Posture | Runtime | Achieved QPS | Avg latency (ms) | Sent | Completed | Lost | Workers |
| --- | --- | --- | --- | --- | --- | --- | --- |
| [dnstap_off](/performance/scenarios.md#feature-tax-dnstap-off-forward-fast) | sync | 77696.7 | 25.7 | 778975 | 778975 | 0 | ingress=2 |
| [dnstap_sampled](/performance/scenarios.md#feature-tax-dnstap-sampled-forward-fast) | sync | 73759.2 | 27.1 | 739578 | 739578 | 0 | ingress=2 |
| [dnstap_full](/performance/scenarios.md#feature-tax-dnstap-full-forward-fast) | sync | 68939.9 | 28.9 | 692386 | 692386 | 0 | ingress=2 |

</div>
<!-- perf-study-evidence:end -->

<!-- perf-study-deltas:start -->
## At a glance

- **dnstap off / sampled / full (forward_fast):** `dnstap_sampled` costs about **5%** QPS versus `dnstap_off` (~74k vs ~78k); `dnstap_full` costs about **11%** QPS versus `dnstap_off` (~69k vs ~78k).
<!-- perf-study-deltas:end -->

## Takeaway

**Dnstap costs scale with how much you emit.** Versus dnstap off on this lab
(~78k), sampled emit costs about **5%** QPS (~74k), and full emit about
**11%** (~69k).

**What to do:** leave dnstap off until you have a consumer. Prefer **sampled**
for standing capture; turn on fuller emit only for surfaces you will store and
query. Configure emit under
[What to emit](/observability/event-export.md#what-to-emit) /
[`events`](/reference/config-schema/events.md). If scrape is also on, see
[Combined metrics + dnstap](/performance/studies/metrics-dnstap-combined-tax.md).

## Related guides

- [Event export](/observability/event-export.md)
- [Event export and dnstap](/guides/event-export-dnstap.md)
- [Reference: events](/reference/config-schema/events.md)
- [Combined metrics + dnstap](/performance/studies/metrics-dnstap-combined-tax.md)
- [Metrics scrape tax](/performance/studies/metrics-scrape-ladder.md)

## Member scenarios

- [feature-tax-dnstap-off-forward-fast](/performance/scenarios.md#feature-tax-dnstap-off-forward-fast)
- [feature-tax-dnstap-sampled-forward-fast](/performance/scenarios.md#feature-tax-dnstap-sampled-forward-fast)
- [feature-tax-dnstap-full-forward-fast](/performance/scenarios.md#feature-tax-dnstap-full-forward-fast)

## Related

- [Studies hub](/performance/studies/index.md)
- [Reference results](/performance/reference.md)
- [Methodology](/performance/methodology.md)
