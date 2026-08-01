# Cache hit vs forward_fast

How much does a warm lookup cache change throughput versus [`forward_fast`](/performance/methodology.md#load-shapes) under
[`sync`](/concepts/runtime-and-concurrency.md#sync-runtime-default)?

Numbers are same-host comparisons (relative to baselines measured on one named
lab profile) and are **not** service-level objectives. See the
[performance hub disclaimer](/performance/index.md).

## When this matters

[Cache-hit](/guides/dns-answer-cache.md) traffic is a different performance regime
than forwarding. Use this pair to bound expectations when the
[lookup cache](/concepts/architecture-and-packet-path.md#lookup) is effective, not
as a claim that production traffic is mostly hits. Configure instances under
[`caches:`](/reference/config-schema/caches.md) and wire them through
[`lookup` profiles](/reference/config-schema/lookup.md).

## What we varied

- **Varied:** load shape
  ([`forward_fast`](/performance/scenarios.md#scale-sync-forward-fast) vs warmed
  [`cache_hit`](/performance/scenarios.md#scale-sync-cache-hit))
- **Held constant:** [`sync`](/concepts/runtime-and-concurrency.md#sync-runtime-default) runtime,
  ingress workers, observability off

<!-- perf-study-evidence:start -->
## Evidence

<div class="perf-chart" markdown="1">

### Achieved QPS — sync cache_hit vs forward_fast

![Achieved QPS — sync cache_hit vs forward_fast](../generated/cache-hit-vs-forward-fast.svg)

[Download CSV](../generated/cache-hit-vs-forward-fast.csv)

| Load shape | Runtime | Achieved QPS | Avg latency (ms) | Sent | Completed | Lost | Workers |
| --- | --- | --- | --- | --- | --- | --- | --- |
| [forward_fast](/performance/scenarios.md#scale-sync-forward-fast) | sync | 211448.9 | 1.3 | 2118239 | 2114785 | 3662 | ingress=2 |
| [cache_hit](/performance/scenarios.md#scale-sync-cache-hit) | sync | 348813.4 | 0.9 | 3491802 | 3488443 | 3366 | ingress=2 |

</div>
<!-- perf-study-evidence:end -->

## Takeaway

On this lab, a warm [`cache_hit`](/performance/methodology.md#load-shapes) path
is about **1.65×** the achieved QPS of
[`forward_fast`](/performance/methodology.md#load-shapes) (lab absolute ~349k vs
~211k) with lower average latency. Treat the multiplier as an upper-bound
illustration when hits dominate — not a portable capacity target or a claim
about your hit rate.

## Related guides

- [DNS answer cache](/guides/dns-answer-cache.md)
- [Architecture and packet path — Lookup](/concepts/architecture-and-packet-path.md#lookup)
- [Reference: caches](/reference/config-schema/caches.md)
- [Reference: lookup](/reference/config-schema/lookup.md)

## Member scenarios

- [scale-sync-forward-fast](/performance/scenarios.md#scale-sync-forward-fast)
- [scale-sync-cache-hit](/performance/scenarios.md#scale-sync-cache-hit)

## Related

- [Studies hub](/performance/studies/index.md)
- [Reference results](/performance/reference.md)
- [Methodology](/performance/methodology.md)
