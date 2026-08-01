# Drain policy under slow upstream

How do complete, budgeted, and minimal drain policies behave under [`forward_slow`](/performance/methodology.md#load-shapes)
load at stop?

Numbers are same-host comparisons (relative to baselines measured on one named
lab profile) and are **not** service-level objectives. See the
[performance hub disclaimer](/performance/index.md).

## When this matters

Shutdown and restart windows trade how long Conduit waits for in-flight work
against client failures during stop. See
[graceful drain on shutdown](/concepts/runtime-and-concurrency.md#graceful-drain-on-shutdown).
Slow upstreams keep transactions outstanding longer, so
[`shutdown.drain` policy](/reference/config-schema/shutdown.md) choice shows up
clearly.

## What we varied

- **Varied:** drain policy
  ([`drain_complete`](/performance/scenarios.md#shutdown-drain-complete-forward-slow),
  [`drain_budgeted`](/performance/scenarios.md#shutdown-drain-budgeted-forward-slow),
  [`drain_minimal`](/performance/scenarios.md#shutdown-drain-minimal-forward-slow))
- **Held constant:** [`forward_slow`](/performance/methodology.md#load-shapes)
  load overlapping SIGTERM, same lab recipe

<!-- perf-study-evidence:start -->
## Evidence

<div class="perf-chart" markdown="1">

### Drain duration under forward_slow

![Drain duration under forward_slow](../generated/drain-duration-forward-slow.svg)

[Download CSV](../generated/drain-duration-forward-slow.csv)

| Drain policy | Drain duration (ms) | Client failures during stop | QPS | Avg latency (ms) |
| --- | --- | --- | --- | --- |
| [drain_complete](/performance/scenarios.md#shutdown-drain-complete-forward-slow) | 113.9 | 300 | 3.2 | 949.9 |
| [drain_budgeted](/performance/scenarios.md#shutdown-drain-budgeted-forward-slow) | 163.6 | 200 | 5.0 | 1022.3 |
| [drain_minimal](/performance/scenarios.md#shutdown-drain-minimal-forward-slow) | 63.8 | 200 | 4.9 | 996.3 |

</div>
<!-- perf-study-evidence:end -->

## Takeaway

On this published reference, **minimal drain finished fastest** (~64 ms),
**complete** took about **114 ms**, and **budgeted** took the longest at about
**164 ms**. That ordering — budgeted slower than complete — is counter to a
naive reading of the policy names (a budget is meant to *bound* drain time, not
extend it) and this is a single-shot, low-volume cell (only a few hundred
queries in flight); treat the budgeted-vs-complete gap as unconfirmed until
remeasured over multiple rounds. **Complete recorded the most client failures
during stop** (300) versus budgeted/minimal (200 each), which is more
consistent with expectations: complete drains longest before giving up, so
requests that were never going to succeed against the slow upstream have more
time to time out and count as failures. **Operator posture:** pick complete,
budgeted, or minimal for your upgrade/restart window from *your* failure budget
— pick a drain policy on purpose rather than inheriting a default you never
reviewed. Vocabulary:
[Performance methodology — Drain policy](/performance/methodology.md#drain-policy-vocabulary).

## Related guides

- [Runtime and concurrency — Graceful drain](/concepts/runtime-and-concurrency.md#graceful-drain-on-shutdown)
- [Shutdown config](/reference/config-schema/shutdown.md)
- [Dataplane runtime tuning](/guides/dataplane-runtime-tuning.md)

## Member scenarios

- [shutdown-drain-complete-forward-slow](/performance/scenarios.md#shutdown-drain-complete-forward-slow)
- [shutdown-drain-budgeted-forward-slow](/performance/scenarios.md#shutdown-drain-budgeted-forward-slow)
- [shutdown-drain-minimal-forward-slow](/performance/scenarios.md#shutdown-drain-minimal-forward-slow)

## Related

- [Studies hub](/performance/studies/index.md)
- [Reference results](/performance/reference.md)
- [Methodology](/performance/methodology.md)
