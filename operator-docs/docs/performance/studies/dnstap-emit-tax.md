# Dnstap emit tax

What does sampled vs fuller dnstap emission cost versus dnstap off?

Numbers are same-host comparisons (relative to baselines measured on one named
lab profile) and are **not** service-level objectives. See the
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
| [dnstap_off](/performance/scenarios.md#feature-tax-dnstap-off-forward-fast) | sync | 135510.5 | 2.2 | 1358795 | 1355390 | 3405 | ingress=2 |
| [dnstap_sampled](/performance/scenarios.md#feature-tax-dnstap-sampled-forward-fast) | sync | 126869.3 | 2.1 | 1272698 | 1269237 | 3461 | ingress=2 |
| [dnstap_full](/performance/scenarios.md#feature-tax-dnstap-full-forward-fast) | sync | 113838.8 | 2.6 | 1142096 | 1138671 | 3425 | ingress=2 |

</div>
<!-- perf-study-evidence:end -->

## Takeaway

On this published reference, **sampled dnstap (~10% of responses) costs about
6%** achieved QPS versus dnstap off, and **full emission costs about 16%**
(lab absolute ~136k / ~127k / ~114k) — costs scale with how much you actually
emit, as expected. **Operator posture:** keep dnstap off until you have a
consumer; prefer **sampled** for standing production capture when full detail
is not required; enable fuller emit only for the surfaces you will actually
store and query. Configure emit surfaces under
[What to emit](/observability/event-export.md#what-to-emit) /
[`events`](/reference/config-schema/events.md). If you also enable scrape, see
[Combined metrics + dnstap](/performance/studies/metrics-dnstap-combined-tax.md).

## Related guides

- [Event export](/observability/event-export.md)
- [Event export and dnstap](/guides/event-export-dnstap.md)
- [Reference: events](/reference/config-schema/events.md)
- [Combined metrics + dnstap](/performance/studies/metrics-dnstap-combined-tax.md)
- [Metrics scrape ladder](/performance/studies/metrics-scrape-ladder.md)

## Member scenarios

- [feature-tax-dnstap-off-forward-fast](/performance/scenarios.md#feature-tax-dnstap-off-forward-fast)
- [feature-tax-dnstap-sampled-forward-fast](/performance/scenarios.md#feature-tax-dnstap-sampled-forward-fast)
- [feature-tax-dnstap-full-forward-fast](/performance/scenarios.md#feature-tax-dnstap-full-forward-fast)

## Related

- [Studies hub](/performance/studies/index.md)
- [Reference results](/performance/reference.md)
- [Methodology](/performance/methodology.md)
