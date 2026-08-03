<div class="perf-chart" markdown="1">

### Achieved QPS — sync cache_hit vs forward_fast

![Achieved QPS — sync cache_hit vs forward_fast](generated/cache-hit-vs-forward-fast.svg)

[Download CSV](generated/cache-hit-vs-forward-fast.csv)

| Load shape | Runtime | Achieved QPS | Avg latency (ms) | Sent | Completed | Lost | Workers |
| --- | --- | --- | --- | --- | --- | --- | --- |
| [forward_fast](/performance/scenarios.md#scale-sync-forward-fast) | sync | 75601.1 | 3.8 | 759747 | 756344 | 3443 | ingress=2 |
| [cache_hit](/performance/scenarios.md#scale-sync-cache-hit) | sync | 333042.1 | 1.0 | 3334080 | 3330738 | 3398 | ingress=2 |

</div>
