# I/O vs ingress (split_io)

<div class="study-question" markdown="1">

With receive and policy threads held fixed on
[`split_io`](/concepts/runtime-and-concurrency.md#split-io-runtime), does adding
more I/O workers improve throughput when the upstream is slow
([`forward_slow`](/performance/methodology.md#load-shapes))?

</div>

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

<!-- perf-study-evidence:start -->
## Evidence

<div class="perf-chart" markdown="1">

### Achieved QPS — split_io io_workers ladder (forward_slow)

_Study figure `io-vs-ingress-split-forward-slow` (io-vs-ingress-split) unavailable — promoted reference lacks member results._

</div>
<!-- perf-study-evidence:end -->

<!-- perf-study-deltas:start -->
## At a glance

- **split_io io_workers ladder (forward_slow):** no published comparison yet (those cells were not promoted).
<!-- perf-study-deltas:end -->

## Takeaway

**There is no published answer yet for this question.** Every
[`forward_slow`](/performance/methodology.md#load-shapes) `io_workers` cell
failed the successful-answer check (too many SERVFAILs), so nothing from this
ladder was promoted. Empty figures are not a ranking.

**What to do:** size `io_workers` from
[Dataplane runtime tuning](/guides/dataplane-runtime-tuning.md), or remeasure
locally with `--study io-vs-ingress-split` until the answer gate passes. Until
then, use the published
[`forward_fast`](/performance/methodology.md#load-shapes) comparison in
[sync vs split_io](/performance/studies/sync-vs-split-io.md) and the sync
ladder in
[ingress concurrency (sync)](/performance/studies/ingress-concurrency-sync.md).

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
