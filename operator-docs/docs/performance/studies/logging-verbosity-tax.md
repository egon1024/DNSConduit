# Logging verbosity tax

<div class="study-question" markdown="1">

What does raising process log level from warn to debug cost under [`forward_fast`](/performance/methodology.md#load-shapes)?

</div>

Numbers are same-host comparisons on a single reference host and are **not**
service-level objectives. See the
[performance hub disclaimer](/performance/index.md).

## When this matters

Other feature_tax cells run with [`logging.level: warn`](/reference/config-schema/logging.md).
Operators sometimes raise verbosity while diagnosing production issues. This
study pairs a dedicated **warn** pole with the same fixture at **debug** under
[`forward_fast`](/performance/methodology.md#load-shapes) (metrics disabled in
both). Pipeline traces use the separate
[`tracing:`](/observability/tracing.md) block — see
[Pipeline tracing tax](/performance/studies/tracing-tax-under-load.md).

## What we varied

- **Varied:** logging level
  ([`logging_warn`](/performance/scenarios.md#feature-tax-logging-warn-forward-fast) vs
  [`logging_debug`](/performance/scenarios.md#feature-tax-logging-debug-forward-fast))
- **Held fixed:** metrics disabled, no dnstap sinks,
  [`sync`](/concepts/runtime-and-concurrency.md#sync-runtime-default),
  [`forward_fast`](/performance/methodology.md#load-shapes)

<!-- perf-study-evidence:start -->
## Evidence

<div class="perf-chart" markdown="1">

### Feature tax — logging warn vs debug (forward_fast)

![Feature tax — logging warn vs debug (forward_fast)](../generated/logging-warn-vs-debug-forward-fast.svg)

[Download CSV](../generated/logging-warn-vs-debug-forward-fast.csv)

| Posture | Runtime | Achieved QPS | Avg latency (ms) | Sent | Completed | Lost | Workers |
| --- | --- | --- | --- | --- | --- | --- | --- |
| [logging_warn](/performance/scenarios.md#feature-tax-logging-warn-forward-fast) | sync | 75892.1 | 4.0 | 762598 | 759249 | 3407 | ingress=2 |
| [logging_debug](/performance/scenarios.md#feature-tax-logging-debug-forward-fast) | sync | 63526.0 | 4.4 | 639007 | 635562 | 3445 | ingress=2 |

</div>
<!-- perf-study-evidence:end -->

<!-- perf-study-deltas:start -->
## At a glance

- **logging warn vs debug (forward_fast):** `logging_debug` costs about **16%** QPS versus `logging_warn` (~64k vs ~76k).
<!-- perf-study-deltas:end -->

## Takeaway

**Debug logging is expensive on the hot path.** On this lab it costs about
**16%** QPS versus warn (~64k vs ~76k) and raises average latency.

**What to do:** keep production at warn/info. Turn debug on only for short
diagnosis windows, then turn it off. For per-query diagnosis, prefer
[pipeline tracing](/observability/tracing.md) with narrow selectors — see
[Pipeline tracing tax](/performance/studies/tracing-tax-under-load.md) for the
cost of leaving tracing wide open.

## Related guides

- [Logging](/observability/logging.md)
- [Reference: logging](/reference/config-schema/logging.md)
- [Pipeline tracing tax](/performance/studies/tracing-tax-under-load.md)

## Member scenarios

- [feature-tax-logging-warn-forward-fast](/performance/scenarios.md#feature-tax-logging-warn-forward-fast)
- [feature-tax-logging-debug-forward-fast](/performance/scenarios.md#feature-tax-logging-debug-forward-fast)

## Related

- [Studies hub](/performance/studies/index.md)
- [Reference results](/performance/reference.md)
- [Methodology](/performance/methodology.md)
