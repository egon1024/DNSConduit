# Memory vs LMDB high-churn cache

<div class="study-question" markdown="1">

Under a matched high-churn recipe with enough sync ingress concurrency for both
backends to do useful parallel work, how do memory and LMDB answer caches differ
in achieved QPS and cache hit/miss ratios?

</div>

Numbers are same-host comparisons on a single reference host and are **not**
service-level objectives. See the
[performance hub disclaimer](/performance/index.md).

## When this matters

Use this pair when you care about **turnover under pressure** — a query set
larger than `max_entries`, stub TTLs long enough that entry-cap eviction (not
TTL expiry alone) drives misses, and continuous fill/evict — not a warm,
hit-dominated path. Both cells use **eight sync ingress workers**, the same
elevated dnsperf window, the same 4096-name query file, a 60 s stub TTL, and
`max_entries: 2048`. LMDB uses `when_full: evict_one`, explicit
`shard_count: 16` (2× ingress), and a real-disk path with a map size sized so
the **entry cap** binds first.

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
(real disk for publish; fixed safe sync default).

## What we varied

- **Varied:** cache backend (memory vs LMDB) under
  [`cache_churn`](/performance/methodology.md#load-shapes)
- **Held constant:** [`sync`](/concepts/runtime-and-concurrency.md#sync-runtime-default)
  runtime with **eight** ingress workers, elevated dnsperf recipe, query file,
  stub TTL, `max_entries`, LMDB shard count on the LMDB cell, metrics scrape
  for hit/miss and fill/eviction path-duration evidence

<!-- perf-study-evidence:start -->
## Evidence

<div class="perf-chart" markdown="1">

### Achieved QPS — sync ingress-8 high-churn (memory vs LMDB)

![Achieved QPS — sync ingress-8 high-churn (memory vs LMDB)](../generated/memory-vs-lmdb-cache-churn-qps.svg)

[Download CSV](../generated/memory-vs-lmdb-cache-churn-qps.csv)

| Cache Backend | Runtime | Achieved QPS | Avg latency (ms) | Sent | Completed | Lost | Workers | Hit rate (%) | Cache hits | Cache misses | Fill mean (ms) | Eviction mean (ms) |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| [memory](/performance/scenarios.md#scale-sync-ingress-8-memory-cache-churn) | sync | 268457.9 | 7.4 | 2687643 | 2687643 | 0 | ingress=8 | 49.7 | 1335770 | 1351874 | 0.0005 | 0.0003 |
| [lmdb](/performance/scenarios.md#scale-sync-ingress-8-lmdb-cache-churn) | sync | 2875.1 | 652.8 | 32244 | 32244 | 0 | ingress=8 | 46.8 | 14368 | 16999 | 2.9683 | 1.6256 |

</div>

<div class="perf-chart" markdown="1">

### Cache hit rate — sync ingress-8 high-churn (memory vs LMDB)

![Cache hit rate — sync ingress-8 high-churn (memory vs LMDB)](../generated/memory-vs-lmdb-cache-churn-hit-rate.svg)

[Download CSV](../generated/memory-vs-lmdb-cache-churn-hit-rate.csv)

| Cache Backend | Hit rate (%) | Cache hits | Cache misses | Achieved QPS |
| --- | --- | --- | --- | --- |
| [memory](/performance/scenarios.md#scale-sync-ingress-8-memory-cache-churn) | 49.7 | 1335770 | 1351874 | 268457.9 |
| [lmdb](/performance/scenarios.md#scale-sync-ingress-8-lmdb-cache-churn) | 46.8 | 14368 | 16999 | 2875.1 |

</div>

<div class="perf-chart" markdown="1">

### Cache fill mean duration — sync ingress-8 high-churn (memory vs LMDB)

![Cache fill mean duration — sync ingress-8 high-churn (memory vs LMDB)](../generated/memory-vs-lmdb-cache-churn-fill-duration.svg)

[Download CSV](../generated/memory-vs-lmdb-cache-churn-fill-duration.csv)

| Cache Backend | Fill mean (ms) | Fill samples | Eviction mean (ms) | Eviction samples | Achieved QPS |
| --- | --- | --- | --- | --- | --- |
| [memory](/performance/scenarios.md#scale-sync-ingress-8-memory-cache-churn) | 0.0005 | 1351874 | 0.0003 | 1343747 | 268457.9 |
| [lmdb](/performance/scenarios.md#scale-sync-ingress-8-lmdb-cache-churn) | 2.9683 | 16999 | 1.6256 | 16360 | 2875.1 |

</div>
<!-- perf-study-evidence:end -->

<!-- perf-study-deltas:start -->
## At a glance

- **sync ingress-8 high-churn (memory vs LMDB):** `lmdb` costs about **99%** QPS versus `memory` (~3k vs ~268k).
- **Cache hit rate — sync ingress-8 high-churn (memory vs LMDB):** `lmdb` is about **6%** lower hit rate than `memory` (~46.8% vs ~49.7%).
- **Cache fill mean duration — sync ingress-8 high-churn (memory vs LMDB):** `lmdb` ≈ ~3.0 ms vs `memory` ~0.0005 ms.
<!-- perf-study-deltas:end -->

## Takeaway

**Under high churn, LMDB is far slower than memory on this lab.** With eight
sync ingress workers and LMDB `shard_count` 16 (fixed safe sync), LMDB costs
about **99%** QPS versus memory (~3k vs ~268k) — roughly **93.4×** slower. That
contrasts with warm [`cache_hit`](/performance/methodology.md#load-shapes), where
LMDB costs only about **6%**. Hit rates stay in the same band (about **6%**
relative); mean fill is about **3.0 ms** for LMDB versus about **0.0005 ms** for
memory. Average latency under the elevated outstanding window tracks roughly
outstanding/QPS and is **not** raw disk service time. Lazy TTL expiry is
deterministic; LMDB `when_full` victim selection is arbitrary under entry
pressure. Absolute LMDB churn QPS is disk and sync-mode sensitive; prefer the
relative claim. See
[Performance methodology — LMDB cache cells](/performance/methodology.md#lmdb-cache-cells).

## Related guides

- [DNS answer cache](/guides/dns-answer-cache.md)
- [Reference: caches](/reference/config-schema/caches.md)
- [Reference: lookup](/reference/config-schema/lookup.md)

## Member scenarios

- [scale-sync-ingress-8-memory-cache-churn](/performance/scenarios.md#scale-sync-ingress-8-memory-cache-churn)
- [scale-sync-ingress-8-lmdb-cache-churn](/performance/scenarios.md#scale-sync-ingress-8-lmdb-cache-churn)

## Related

- [Memory vs LMDB warm cache_hit](/performance/studies/memory-vs-lmdb-cache-hit.md)
- [Cache hit vs forward](/performance/studies/cache-hit-vs-forward.md)
- [Studies hub](/performance/studies/index.md)
- [Reference results](/performance/reference.md)
- [Methodology](/performance/methodology.md)
