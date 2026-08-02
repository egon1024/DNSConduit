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

<!-- perf-study-evidence:start -->
## Evidence

<div class="perf-chart" markdown="1">

### Achieved QPS — sync vs split_io (forward_fast)

![Achieved QPS — sync vs split_io (forward_fast)](../generated/sync-vs-split-io-forward-fast.svg)

[Download CSV](../generated/sync-vs-split-io-forward-fast.csv)

| Runtime | Achieved QPS | Avg latency (ms) | Sent | Completed | Lost | Workers |
| --- | --- | --- | --- | --- | --- | --- |
| [sync](/performance/scenarios.md#scale-sync-forward-fast) | 74932.5 | 3.6 | 753275 | 749881 | 3465 | ingress=2 |
| [split_io](/performance/scenarios.md#scale-split-io-forward-fast) | 140686.6 | 1.4 | 1424907 | 1421028 | 3879 | ingress=2, policy=2, io=2 |

</div>

<div class="perf-chart" markdown="1">

### Achieved QPS — sync vs split_io (forward_slow)

![Achieved QPS — sync vs split_io (forward_slow)](../generated/sync-vs-split-io-forward-slow.svg)

[Download CSV](../generated/sync-vs-split-io-forward-slow.csv)

| Runtime | Achieved QPS | Avg latency (ms) | Sent | Completed | Lost | Workers |
| --- | --- | --- | --- | --- | --- | --- |
| [sync](/performance/scenarios.md#scale-sync-forward-slow) | 5.7 | 2509.5 | 12188 | 198 | 11990 | ingress=2 |
| — | — | — | — | — | — | — |

</div>
<!-- perf-study-evidence:end -->

<!-- perf-study-deltas:start -->
## At a glance

- **sync vs split_io (forward_fast):** `split_io` is about **1.9×** `sync` (~141k vs ~75k).
- **sync vs split_io (forward_slow):** only `sync` is published (~6 QPS); no paired comparison on this reference.
<!-- perf-study-deltas:end -->

## Takeaway

**Against a fast upstream, `split_io` outperforms `sync` on this lab.** Under
[`forward_fast`](/performance/methodology.md#load-shapes),
[`split_io`](/concepts/runtime-and-concurrency.md#split-io-runtime) reaches about
**1.9×** the QPS of
[`sync`](/concepts/runtime-and-concurrency.md#sync-runtime-default) (~141k vs
~75k) with lower average latency and little query loss.

**This page does not yet rank the two under a slow upstream.** Only the sync
[`forward_slow`](/performance/methodology.md#load-shapes) pole is published
(~6 QPS, high loss). The paired `split_io` cell failed the successful-answer
check and was omitted.

**What to do:** prefer `split_io` when the fast-path delta matters, then size
workers with
[Dataplane runtime tuning](/guides/dataplane-runtime-tuning.md) and
[ingress concurrency (sync)](/performance/studies/ingress-concurrency-sync.md).
For slow backends, start from `split_io` by design and remeasure
[I/O vs ingress (split_io)](/performance/studies/io-vs-ingress-split.md) once a
clean slow-path cell is available.

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
