<div class="perf-chart" markdown="1">

### Achieved QPS — sync cache_hit vs forward_fast

![Achieved QPS — sync cache_hit vs forward_fast](generated/cache-hit-vs-forward-fast.svg)

[Download CSV](generated/cache-hit-vs-forward-fast.csv)

| Load shape | Runtime | Achieved QPS | Avg latency (ms) | Sent | Completed | Lost | Workers |
| --- | --- | --- | --- | --- | --- | --- | --- |
| [forward_fast](/performance/scenarios.md#scale-sync-forward-fast) | sync | 73594.4 | 27.1 | 738415 | 738415 | 0 | ingress=2 |
| [cache_hit](/performance/scenarios.md#scale-sync-cache-hit) | sync | 254403.3 | 7.8 | 2546522 | 2546522 | 0 | ingress=2 |

</div>
