## Evidence

<div class="perf-chart" markdown="1">

### Achieved QPS — sync cache_hit vs forward_fast

![Achieved QPS — sync cache_hit vs forward_fast](generated/cache-hit-vs-forward-fast.svg)

[Download CSV](generated/cache-hit-vs-forward-fast.csv)

| Load shape | Runtime | Achieved QPS | Avg latency (ms) | Sent | Completed | Lost | Workers |
| --- | --- | --- | --- | --- | --- | --- | --- |
| [forward_fast](/performance/scenarios.md#scale-sync-forward-fast) | sync | 211448.9 | 1.3 | 2118239 | 2114785 | 3662 | ingress=2 |
| [cache_hit](/performance/scenarios.md#scale-sync-cache-hit) | sync | 348813.4 | 0.9 | 3491802 | 3488443 | 3366 | ingress=2 |

</div>
