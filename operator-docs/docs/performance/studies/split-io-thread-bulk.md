# Split_io bulk thread topology

Does raising ingress, policy, and I/O worker counts together always help under
[`forward_fast`](/performance/methodology.md#load-shapes)?

Numbers are same-host comparisons (relative to baselines measured on one named
lab profile) and are **not** service-level objectives. See the
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
| [thin](/performance/scenarios.md#scale-split-io-forward-fast) | split_io | 364056.4 | 2.2 | 3644026 | 3642237 | 2395 | ingress=2, policy=2, io=2 |
| [heavy](/performance/scenarios.md#scale-split-io-topology-heavy) | split_io | 601897.6 | 2.7 | 6021233 | 6020309 | 611 | ingress=4, policy=4, io=4 |

</div>
<!-- perf-study-evidence:end -->

## Takeaway

On this published reference, doubling all three worker counts together (4/4/4)
delivers about **1.65×** the achieved QPS of the modest baseline (2/2/2) (lab
absolute ~602k vs ~364k QPS). With enough offered concurrency to actually
exercise the extra workers (see
[How load is applied](/performance/methodology.md#how-load-is-applied-not-a-fixed-offered-qps)),
raising every `split_io` worker pool together **does** buy meaningful headroom
here — it is not automatically wasted effort. That does not make "double
everything" a substitute for sizing: this cell does not show whether ingress,
policy, or I/O workers individually explain the gain, whether 8/8/8 keeps
scaling or plateaus, or how the picture changes on your own upstream latency
and concurrency profile. Size **one** setting at a time with
[Dataplane runtime tuning](/guides/dataplane-runtime-tuning.md) and the studies
that vary a single worker count before committing to a topology.

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
