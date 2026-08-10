# Memory vs LMDB high-churn cache

<div class="study-question" markdown="1">

Under a matched high-churn recipe with enough sync ingress concurrency for both
backends to do useful parallel work, how do memory and LMDB answer caches differ
in achieved QPS and cache hit/miss ratios — and how do the four LMDB sync
durability modes (`full`, `no_meta`, `periodic`, `none`) compare under that same
recipe?

</div>

Numbers are same-host comparisons on a single reference host and are **not**
service-level objectives. See the
[performance hub disclaimer](/performance/index.md).

## When this matters

Use this study when you care about **turnover under pressure** — a query set
larger than `max_entries`, stub TTLs long enough that entry-cap eviction (not
TTL expiry alone) drives misses, and continuous fill/evict — not a warm,
hit-dominated path. Members use **eight sync ingress workers**, the same
elevated dnsperf window, the same 4096-name query file, a 60 s stub TTL, and
`max_entries: 2048`. LMDB cells use `when_full: evict_one`, explicit
`shard_count: 16` (2× ingress), distinct real-disk paths, and first-class
`lmdb.sync` values (`full`, `no_meta`, `periodic`, `none`) with a map size sized
so the **entry cap** binds first. The `periodic` cell uses `sync_interval: 1s`.

This shape is intentional: a two-worker sync path under the same elevated
outstanding window mainly shows Little's Law queueing (avg latency ≈
outstanding ÷ QPS) and starves multi-env LMDB parallelism. Thin-ingress
companion scenarios remain in the catalog but are **not** this study’s
primary compare.

This study does **not** describe warm read-mostly `cache_hit` cost — see
[Memory vs LMDB warm cache_hit](/performance/studies/memory-vs-lmdb-cache-hit.md).

**Expiry vs eviction:** lazy TTL expiry is wall-clock deterministic on read.
What is arbitrary under LMDB capacity pressure is `when_full` victim selection
(`evict_one` / `sample`), not the expiry clock.

Configure backends under [`caches:`](/reference/config-schema/caches.md) and
[`lookup` profiles](/reference/config-schema/lookup.md). See
[DNS answer cache](/guides/dns-answer-cache.md) and
[Performance methodology — LMDB cache cells](/performance/methodology.md#lmdb-cache-cells)
(real disk for publish; each LMDB cell annotates its `lmdb.sync` value).

## What we varied

- **Varied:** cache backend (memory vs LMDB) and LMDB **`lmdb.sync`** mode under
  [`cache_churn`](/performance/methodology.md#load-shapes)
- **Held constant:** [`sync`](/concepts/runtime-and-concurrency.md#sync-runtime-default)
  runtime with **eight** ingress workers, elevated dnsperf recipe, query file,
  stub TTL, `max_entries`, LMDB shard count on LMDB cells, metrics scrape
  for hit/miss and fill/eviction path-duration evidence

<!-- perf-study-evidence:start -->
## Evidence

<div class="perf-chart" markdown="1">

### Achieved QPS — sync ingress-8 high-churn (memory vs LMDB sync modes)

![Achieved QPS — sync ingress-8 high-churn (memory vs LMDB sync modes)](../generated/memory-vs-lmdb-cache-churn-qps.svg)

[Download CSV](../generated/memory-vs-lmdb-cache-churn-qps.csv)

| Lmdb Sync | Runtime | Achieved QPS | Avg latency (ms) | Sent | Completed | Lost | Workers | Hit rate (%) | Cache hits | Cache misses | Fill mean (ms) | Eviction mean (ms) |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| [memory](/performance/scenarios.md#scale-sync-ingress-8-memory-cache-churn) | sync | 232253.7 | 8.6 | 2325790 | 2325790 | 0 | ingress=8 | 49.7 | 1154131 | 1171660 | 0.0005 | 0.0003 |
| [full](/performance/scenarios.md#scale-sync-ingress-8-lmdb-full-cache-churn) | sync | 2767.2 | 698.4 | 29235 | 29235 | 0 | ingress=8 | 42.1 | 12317 | 16919 | 2.8751 | 1.6619 |
| [no_meta](/performance/scenarios.md#scale-sync-ingress-8-lmdb-no_meta-cache-churn) | sync | 4379.2 | 442.1 | 46180 | 46180 | 0 | ingress=8 | 45.0 | 20520 | 25094 | 1.9188 | 1.0409 |
| [periodic](/performance/scenarios.md#scale-sync-ingress-8-lmdb-periodic-cache-churn) | sync | 191677.2 | 10.4 | 1920116 | 1920116 | 0 | ingress=8 | 50.2 | 964032 | 956085 | 0.0129 | 0.0068 |
| [none](/performance/scenarios.md#scale-sync-ingress-8-lmdb-none-cache-churn) | sync | 176805.2 | 11.3 | 1771208 | 1771208 | 0 | ingress=8 | 50.0 | 886097 | 885112 | 0.0124 | 0.0065 |

</div>

<div class="perf-chart" markdown="1">

### Cache hit rate — sync ingress-8 high-churn (memory vs LMDB sync modes)

![Cache hit rate — sync ingress-8 high-churn (memory vs LMDB sync modes)](../generated/memory-vs-lmdb-cache-churn-hit-rate.svg)

[Download CSV](../generated/memory-vs-lmdb-cache-churn-hit-rate.csv)

| Lmdb Sync | Hit rate (%) | Cache hits | Cache misses | Achieved QPS |
| --- | --- | --- | --- | --- |
| [memory](/performance/scenarios.md#scale-sync-ingress-8-memory-cache-churn) | 49.7 | 1154131 | 1171660 | 232253.7 |
| [full](/performance/scenarios.md#scale-sync-ingress-8-lmdb-full-cache-churn) | 42.1 | 12317 | 16919 | 2767.2 |
| [no_meta](/performance/scenarios.md#scale-sync-ingress-8-lmdb-no_meta-cache-churn) | 45.0 | 20520 | 25094 | 4379.2 |
| [periodic](/performance/scenarios.md#scale-sync-ingress-8-lmdb-periodic-cache-churn) | 50.2 | 964032 | 956085 | 191677.2 |
| [none](/performance/scenarios.md#scale-sync-ingress-8-lmdb-none-cache-churn) | 50.0 | 886097 | 885112 | 176805.2 |

</div>

<div class="perf-chart" markdown="1">

### Cache fill mean duration — sync ingress-8 high-churn (memory vs LMDB sync modes)

![Cache fill mean duration — sync ingress-8 high-churn (memory vs LMDB sync modes)](../generated/memory-vs-lmdb-cache-churn-fill-duration.svg)

[Download CSV](../generated/memory-vs-lmdb-cache-churn-fill-duration.csv)

| Lmdb Sync | Fill mean (ms) | Fill samples | Eviction mean (ms) | Eviction samples | Achieved QPS |
| --- | --- | --- | --- | --- | --- |
| [memory](/performance/scenarios.md#scale-sync-ingress-8-memory-cache-churn) | 0.0005 | 1171660 | 0.0003 | 1164824 | 232253.7 |
| [full](/performance/scenarios.md#scale-sync-ingress-8-lmdb-full-cache-churn) | 2.8751 | 16919 | 1.6619 | 14884 | 2767.2 |
| [no_meta](/performance/scenarios.md#scale-sync-ingress-8-lmdb-no_meta-cache-churn) | 1.9188 | 25094 | 1.0409 | 23629 | 4379.2 |
| [periodic](/performance/scenarios.md#scale-sync-ingress-8-lmdb-periodic-cache-churn) | 0.0129 | 956085 | 0.0068 | 954046 | 191677.2 |
| [none](/performance/scenarios.md#scale-sync-ingress-8-lmdb-none-cache-churn) | 0.0124 | 885112 | 0.0065 | 883086 | 176805.2 |

</div>
<!-- perf-study-evidence:end -->

<!-- perf-study-deltas:start -->
## At a glance

- **sync ingress-8 high-churn (memory vs LMDB sync modes):** `full` costs about **99%** QPS versus `memory` (~3k vs ~232k); `no_meta` costs about **98%** QPS versus `memory` (~4k vs ~232k); `periodic` costs about **17%** QPS versus `memory` (~192k vs ~232k); `none` costs about **24%** QPS versus `memory` (~177k vs ~232k).
- **Cache hit rate — sync ingress-8 high-churn (memory vs LMDB sync modes):** `full` is about **15%** lower hit rate than `memory` (~42.1% vs ~49.7%); `no_meta` is about **9%** lower hit rate than `memory` (~45.0% vs ~49.7%); `periodic` is about **1%** higher hit rate than `memory` (~50.2% vs ~49.7%); `none` is about **1%** higher hit rate than `memory` (~50.0% vs ~49.7%).
- **Cache fill mean duration — sync ingress-8 high-churn (memory vs LMDB sync modes):** `full` ≈ ~2.9 ms vs `memory` ~0.0005 ms; `no_meta` ≈ ~1.9 ms vs `memory` ~0.0005 ms; `periodic` is about **25.8×** `memory` (~0.013 ms vs ~0.0005 ms); `none` is about **24.8×** `memory` (~0.012 ms vs ~0.0005 ms).
<!-- perf-study-deltas:end -->

## Takeaway

**Under high churn, LMDB write durability dominates QPS on this lab.** With eight
sync ingress workers and LMDB `shard_count` 16, **`sync: full`** costs about
**99%** QPS versus memory (~3k vs ~232k; roughly **83.9×** slower). Moving to
**`no_meta`** is only about **1.6×** `full` (~4k) — a modest write-path gain.
**`periodic`** (this cell uses `sync_interval: 1s`) is about **43.8×** `no_meta`
(~192k) and costs about **17%** QPS versus memory. **`none`** lands in the same
high-QPS band (~177k; about **24%** versus memory) but skips forced syncs
entirely — on this lab `periodic` was about **1.1×** `none`. Hit rates stay in
the same band (`full` about **15%** relative lower than memory). Mean fill is
about **2.9 ms** for `full`, about **1.9 ms** for `no_meta`, and about
**0.013 ms** for `periodic` and `none`, versus about **0.0005 ms** for memory.
Average latency under the elevated outstanding window tracks roughly
outstanding/QPS and is **not** raw disk service time. Lazy TTL expiry is
deterministic; LMDB `when_full` victim selection is arbitrary under entry
pressure. Absolute LMDB churn QPS is disk- and sync-mode-sensitive; prefer
relative claims. That contrasts with warm
[`cache_hit`](/performance/methodology.md#load-shapes), where LMDB costs only
about **6%** versus memory — see
[Memory vs LMDB warm cache_hit](/performance/studies/memory-vs-lmdb-cache-hit.md).

**Before choosing a faster LMDB sync mode for production, you must understand
durability and integrity tradeoffs.** Start from the
[`lmdb.sync` decision tree](/reference/config-schema/caches.md#lmdb-sync) and the
[DNS answer cache](/guides/dns-answer-cache.md) guide — do not pick a mode from
the QPS chart alone. See also
[Performance methodology — LMDB cache cells](/performance/methodology.md#lmdb-cache-cells).

## Related guides

- [DNS answer cache](/guides/dns-answer-cache.md)
- [Reference: caches](/reference/config-schema/caches.md)
- [Reference: lookup](/reference/config-schema/lookup.md)

## Member scenarios

- [scale-sync-ingress-8-memory-cache-churn](/performance/scenarios.md#scale-sync-ingress-8-memory-cache-churn)
- [scale-sync-ingress-8-lmdb-full-cache-churn](/performance/scenarios.md#scale-sync-ingress-8-lmdb-full-cache-churn)
- [scale-sync-ingress-8-lmdb-no_meta-cache-churn](/performance/scenarios.md#scale-sync-ingress-8-lmdb-no_meta-cache-churn)
- [scale-sync-ingress-8-lmdb-periodic-cache-churn](/performance/scenarios.md#scale-sync-ingress-8-lmdb-periodic-cache-churn)
- [scale-sync-ingress-8-lmdb-none-cache-churn](/performance/scenarios.md#scale-sync-ingress-8-lmdb-none-cache-churn)

## Related

- [Memory vs LMDB warm cache_hit](/performance/studies/memory-vs-lmdb-cache-hit.md)
- [Cache hit vs forward](/performance/studies/cache-hit-vs-forward.md)
- [Studies hub](/performance/studies/index.md)
- [Reference results](/performance/reference.md)
- [Methodology](/performance/methodology.md)
