## Evidence

<div class="perf-chart" markdown="1">

### Achieved QPS — sync cache_hit vs forward_fast

![Achieved QPS — sync cache_hit vs forward_fast](generated/cache-hit-vs-forward-fast.svg)

[Download CSV](generated/cache-hit-vs-forward-fast.csv)

| Load shape | Runtime | Achieved QPS | Avg latency (ms) | Sent | Completed | Lost | Workers |
| --- | --- | --- | --- | --- | --- | --- | --- |
| [forward_fast](/performance/scenarios.md#scale-sync-forward-fast) | sync | 76269.9 | 26.1 | 765379 | 765379 | 0 | ingress=2 |
| [cache_hit](/performance/scenarios.md#scale-sync-cache-hit) | sync | 329202.8 | 6.1 | 3294253 | 3294253 | 0 | ingress=2 |

</div>
