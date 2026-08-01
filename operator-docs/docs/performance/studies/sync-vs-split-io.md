# Sync vs split_io

How does `dataplane.runtime:` [`sync`](/concepts/runtime-and-concurrency.md#sync-runtime-default)
compare to [`split_io`](/concepts/runtime-and-concurrency.md#split-io-runtime) under
[`forward_fast`](/performance/methodology.md#load-shapes) and
[`forward_slow`](/performance/methodology.md#load-shapes)?

Numbers are same-host comparisons (relative to baselines measured on one named
lab profile) and are **not** service-level objectives. See the
[performance hub disclaimer](/performance/index.md).

## When this matters

Choosing a [runtime model](/concepts/runtime-and-concurrency.md) changes how
ingress and upstream I/O share threads.
[**`sync`**](/concepts/runtime-and-concurrency.md#sync-runtime-default) keeps
receive and forward on the same workers;
[`split_io`](/concepts/runtime-and-concurrency.md#split-io-runtime) separates
ingress from async I/O workers. Architecture suggests slow upstreams can stall
[`sync`](/concepts/runtime-and-concurrency.md#sync-runtime-default) ingress harder than
[`split_io`](/concepts/runtime-and-concurrency.md#split-io-runtime); this study checks
that claim under paired load shapes on one lab. See also
[worker counts](/concepts/runtime-and-concurrency.md#worker-counts-and-limits)
and the [dataplane runtime tuning](/guides/dataplane-runtime-tuning.md) guide.

## What we varied

- **Varied:** `dataplane.runtime`
  ([`sync`](/performance/scenarios.md#scale-sync-forward-fast) vs
  [`split_io`](/performance/scenarios.md#scale-split-io-forward-fast); paired
  slow runs under [Member scenarios](#member-scenarios))
- **Held fixed:** same dnsperf recipe, same named lab profile, observability off
  fixtures
- **Two load shapes:** [`forward_fast`](/performance/methodology.md#load-shapes) and
  [`forward_slow`](/performance/methodology.md#load-shapes)

!!! warning "Stressed `forward_slow` evidence in this reference"
    The `forward_slow` table below is **lossy and low QPS** (~7–10 achieved QPS
    with high Lost). It is **not** a clean ranking of `sync` vs `split_io`. Prefer
    the `forward_fast` figure for a same-host delta; see
    [How load is applied](/performance/methodology.md#how-load-is-applied-not-a-fixed-offered-qps).

<!-- perf-study-evidence:start -->
## Evidence

<div class="perf-chart" markdown="1">

### Achieved QPS — sync vs split_io (forward_fast)

![Achieved QPS — sync vs split_io (forward_fast)](../generated/sync-vs-split-io-forward-fast.svg)

[Download CSV](../generated/sync-vs-split-io-forward-fast.csv)

| Runtime | Achieved QPS | Avg latency (ms) | Sent | Completed | Lost | Workers |
| --- | --- | --- | --- | --- | --- | --- |
| [sync](/performance/scenarios.md#scale-sync-forward-fast) | 211448.9 | 1.3 | 2118239 | 2114785 | 3662 | ingress=2 |
| [split_io](/performance/scenarios.md#scale-split-io-forward-fast) | 364056.4 | 2.2 | 3644026 | 3642237 | 2395 | ingress=2, policy=2, io=2 |

</div>

<div class="perf-chart" markdown="1">

### Achieved QPS — sync vs split_io (forward_slow)

![Achieved QPS — sync vs split_io (forward_slow)](../generated/sync-vs-split-io-forward-slow.svg)

[Download CSV](../generated/sync-vs-split-io-forward-slow.csv)

| Runtime | Achieved QPS | Avg latency (ms) | Sent | Completed | Lost | Workers |
| --- | --- | --- | --- | --- | --- | --- |
| [sync](/performance/scenarios.md#scale-sync-forward-slow) | 10.2 | 1651.1 | 340 | 153 | 187 | ingress=2 |
| [split_io](/performance/scenarios.md#scale-split-io-forward-slow) | 10.9 | 3320.2 | 298 | 161 | 137 | ingress=2, policy=2, io=2 |

</div>
<!-- perf-study-evidence:end -->

## Takeaway

On this lab profile under [`forward_fast`](/performance/methodology.md#load-shapes),
[`split_io`](/concepts/runtime-and-concurrency.md#split-io-runtime) delivers about
**1.7×** the achieved QPS of
[`sync`](/concepts/runtime-and-concurrency.md#sync-runtime-default) (lab absolute
~364k vs ~211k); Loss stays small relative to Sent.
The [`forward_slow`](/performance/methodology.md#load-shapes) pair is **not** a
clean ranking — both stay around ~7–10 QPS with high query loss (upstream delay
dominating). **Operator posture:** prefer the `forward_fast` delta when choosing
a runtime; if upstreams are slow or variable, start from `split_io` and size
workers with
[Dataplane runtime tuning](/guides/dataplane-runtime-tuning.md),
[ingress concurrency (sync)](/performance/studies/ingress-concurrency-sync.md),
or [I/O vs ingress (split_io)](/performance/studies/io-vs-ingress-split.md) — do
not rank models from the stressed slow cells alone.

## Related guides

- [Runtime and concurrency](/concepts/runtime-and-concurrency.md) — sync vs split_io models
- [Dataplane runtime tuning](/guides/dataplane-runtime-tuning.md)
- [Reference: dataplane](/reference/config-schema/dataplane.md) — `runtime`, `io_workers`, `policy_workers`
- [Ingress concurrency (sync)](/performance/studies/ingress-concurrency-sync.md)
- [I/O vs ingress (split_io)](/performance/studies/io-vs-ingress-split.md)

## Member scenarios

- [scale-sync-forward-fast](/performance/scenarios.md#scale-sync-forward-fast)
- [scale-split-io-forward-fast](/performance/scenarios.md#scale-split-io-forward-fast)
- [scale-sync-forward-slow](/performance/scenarios.md#scale-sync-forward-slow)
- [scale-split-io-forward-slow](/performance/scenarios.md#scale-split-io-forward-slow)
