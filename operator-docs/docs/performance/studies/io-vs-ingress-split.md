# I/O vs ingress (split_io)

With receive and policy threads held fixed on
[`split_io`](/concepts/runtime-and-concurrency.md#split-io-runtime), does adding
more I/O workers improve throughput when the upstream is slow
([`forward_slow`](/performance/methodology.md#load-shapes))?

Numbers are same-host comparisons (relative to baselines measured on one named
lab profile) and are **not** service-level objectives. See the
[performance hub disclaimer](/performance/index.md).

## When this matters

On **[`split_io`](/concepts/runtime-and-concurrency.md#split-io-runtime)**,
ingress and async I/O are separate roles.
[`dataplane.io_workers`](/reference/config-schema/dataplane.md) sizes the I/O
pool that owns upstream wait; listener
[`threads`](/reference/config-schema/listeners.md) stay the receive path.
This study asks whether adding I/O workers under
[`forward_slow`](/performance/methodology.md#load-shapes) moves QPS when
ingress/policy are fixed. See
[worker counts](/concepts/runtime-and-concurrency.md#worker-counts-and-limits)
and [dataplane runtime tuning](/guides/dataplane-runtime-tuning.md).

## What we varied

- **Varied:** `dataplane.io_workers`
  ([`1`](/performance/scenarios.md#scale-split-io-io-1-forward-slow),
  [`2`](/performance/scenarios.md#scale-split-io-forward-slow),
  [`4`](/performance/scenarios.md#scale-split-io-io-4-forward-slow),
  [`8`](/performance/scenarios.md#scale-split-io-io-8-forward-slow)) with
  `dataplane.runtime:` [`split_io`](/concepts/runtime-and-concurrency.md#split-io-runtime)
  and fixed ingress (`threads: 2`)
- **Held fixed:** [`forward_slow`](/performance/methodology.md#load-shapes) load
  shape, observability off fixtures, same dnsperf recipe on the named lab profile
- **Note:** `io=2` reuses the existing split_io `forward_slow` scale cell

!!! warning "Stressed / inconclusive ladder in this reference"
    These `forward_slow` cells are **lossy and low QPS** across `io_workers` 1–8.
    Do not treat them as proof that I/O count never matters under a fast or
    moderately slow upstream — remeasure under your load. Background:
    [How load is applied](/performance/methodology.md#how-load-is-applied-not-a-fixed-offered-qps).

<!-- perf-study-evidence:start -->
## Evidence

<div class="perf-chart" markdown="1">

### Achieved QPS — split_io io_workers ladder (forward_slow)

![Achieved QPS — split_io io_workers ladder (forward_slow)](../generated/io-vs-ingress-split-forward-slow.svg)

[Download CSV](../generated/io-vs-ingress-split-forward-slow.csv)

| I/O workers | Runtime | Achieved QPS | Avg latency (ms) | Sent | Completed | Lost | Workers |
| --- | --- | --- | --- | --- | --- | --- | --- |
| [1](/performance/scenarios.md#scale-split-io-io-1-forward-slow) | split_io | 10.5 | 2751.1 | 297 | 145 | 151 | ingress=2, policy=2, io=1 |
| [2](/performance/scenarios.md#scale-split-io-forward-slow) | split_io | 10.9 | 3320.2 | 298 | 161 | 137 | ingress=2, policy=2, io=2 |
| [4](/performance/scenarios.md#scale-split-io-io-4-forward-slow) | split_io | 9.0 | 3152.0 | 299 | 134 | 164 | ingress=2, policy=2, io=4 |
| [8](/performance/scenarios.md#scale-split-io-io-8-forward-slow) | split_io | 8.9 | 3148.5 | 299 | 133 | 165 | ingress=2, policy=2, io=8 |

</div>
<!-- perf-study-evidence:end -->

## Takeaway

On this lab under stressed [`forward_slow`](/performance/methodology.md#load-shapes),
**raising `io_workers` does not unlock throughput** — all poles stay within a
narrow band (~8–10 QPS) with high Lost. Upstream delay and the loadgen
outstanding window dominate; I/O thread count is not the bottleneck, so a small
downward drift at higher counts is **not** a claim that more workers hurt.
**Operator posture:** start with a small I/O pool (often `io_workers: 1` per
[Dataplane runtime tuning](/guides/dataplane-runtime-tuning.md)); raise it only
when I/O is saturated under a realistic upstream, not under this stressed recipe.
If your results are also flat, look at
[slot / inflight caps](/concepts/runtime-and-concurrency.md#transaction-slot-pool)
or backend capacity instead. Contrast with
[ingress concurrency (sync)](/performance/studies/ingress-concurrency-sync.md)
and [sync vs split_io](/performance/studies/sync-vs-split-io.md).

## Related guides

- [Runtime and concurrency](/concepts/runtime-and-concurrency.md) — split_io model and worker roles
- [Dataplane runtime tuning](/guides/dataplane-runtime-tuning.md)
- [Reference: dataplane](/reference/config-schema/dataplane.md) — `runtime`, `io_workers`, `policy_workers`
- [Reference: listeners](/reference/config-schema/listeners.md) — ingress `threads`
- [Ingress concurrency (sync)](/performance/studies/ingress-concurrency-sync.md)
- [Sync vs split_io](/performance/studies/sync-vs-split-io.md)

## Member scenarios

- [scale-split-io-io-1-forward-slow](/performance/scenarios.md#scale-split-io-io-1-forward-slow)
- [scale-split-io-forward-slow](/performance/scenarios.md#scale-split-io-forward-slow)
- [scale-split-io-io-4-forward-slow](/performance/scenarios.md#scale-split-io-io-4-forward-slow)
- [scale-split-io-io-8-forward-slow](/performance/scenarios.md#scale-split-io-io-8-forward-slow)
