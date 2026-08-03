# Split_io bulk thread topology

<div class="study-question" markdown="1">

Does raising ingress, policy, and I/O worker counts together always help under
[`forward_fast`](/performance/methodology.md#load-shapes)?

</div>

Numbers are same-host comparisons on a single reference host and are **not**
service-level objectives. See the
[performance hub disclaimer](/performance/index.md).

## When this matters

It is tempting to raise all [`split_io`](/concepts/runtime-and-concurrency.md#split-io-runtime)
worker counts at once (`listeners.threads`, `dataplane.policy_workers`,
`dataplane.io_workers` — see
[worker counts](/concepts/runtime-and-concurrency.md#worker-counts-and-limits)).
This study compares a modest baseline (2 of each) with doubling all three
together (4 of each) under [`forward_fast`](/performance/methodology.md#load-shapes).
To see which setting actually helps, change **one** count at a time:
[I/O vs ingress (split_io)](/performance/studies/io-vs-ingress-split.md) and
[ingress concurrency (sync)](/performance/studies/ingress-concurrency-sync.md).

## What we varied

- **Varied:** all three worker counts together
  ([2/2/2 baseline](/performance/scenarios.md#scale-split-io-forward-fast) vs
  [4/4/4 doubled](/performance/scenarios.md#scale-split-io-topology-heavy))
- **Held constant:** [`split_io`](/concepts/runtime-and-concurrency.md#split-io-runtime),
  [`forward_fast`](/performance/methodology.md#load-shapes), observability off

<!-- perf-study-evidence:start -->
## Evidence

<div class="perf-chart" markdown="1">

### Achieved QPS — split_io topology (forward_fast)

![Achieved QPS — split_io topology (forward_fast)](../generated/split-io-topology-bulk-forward-fast.svg)

[Download CSV](../generated/split-io-topology-bulk-forward-fast.csv)

| Topology | Runtime | Achieved QPS | Avg latency (ms) | Sent | Completed | Lost | Workers |
| --- | --- | --- | --- | --- | --- | --- | --- |
| [thin](/performance/scenarios.md#scale-split-io-forward-fast) | split_io | 147142.3 | 1.2 | 1479133 | 1475210 | 3912 | ingress=2, policy=2, io=2 |
| — | — | — | — | — | — | — | — |

</div>
<!-- perf-study-evidence:end -->

<!-- perf-study-deltas:start -->
## At a glance

- **split_io topology (forward_fast):** only `thin` is published (~147k); no paired comparison on this reference.
<!-- perf-study-deltas:end -->

## Takeaway

**This page does not yet answer whether doubling all worker pools helps.** Only
the modest baseline (2/2/2 at ~147k QPS) is published. The topology-heavy
(4/4/4) cell failed the successful-answer check and was omitted — so there is
no bulk-topology multiplier here.

**What to do:** do not treat “double everything” as proven. Size **one**
setting at a time with
[Dataplane runtime tuning](/guides/dataplane-runtime-tuning.md) and the
single-axis studies; remeasure the bulk pair locally once it clears the answer
gate.

## Related guides

- [Runtime and concurrency](/concepts/runtime-and-concurrency.md)
- [Dataplane runtime tuning](/guides/dataplane-runtime-tuning.md)
- [Reference: dataplane](/reference/config-schema/dataplane.md)
- [I/O vs ingress (split_io)](/performance/studies/io-vs-ingress-split.md)
- [Sync vs split_io](/performance/studies/sync-vs-split-io.md)

## Member scenarios

- [scale-split-io-forward-fast](/performance/scenarios.md#scale-split-io-forward-fast)
- [scale-split-io-topology-heavy](/performance/scenarios.md#scale-split-io-topology-heavy)

## Related

- [Studies hub](/performance/studies/index.md)
- [Reference results](/performance/reference.md)
- [Methodology](/performance/methodology.md)
