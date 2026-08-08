<div class="perf-chart" markdown="1">

### Achieved QPS — sync warm cache_hit (memory vs LMDB)

![Achieved QPS — sync warm cache_hit (memory vs LMDB)](generated/memory-vs-lmdb-cache-hit-qps.svg)

[Download CSV](generated/memory-vs-lmdb-cache-hit-qps.csv)

| Cache Backend | Runtime | Achieved QPS | Avg latency (ms) | Sent | Completed | Lost | Workers |
| --- | --- | --- | --- | --- | --- | --- | --- |
| [memory](/performance/scenarios.md#scale-sync-cache-hit) | sync | 329202.8 | 6.1 | 3294253 | 3294253 | 0 | ingress=2 |
| [lmdb](/performance/scenarios.md#scale-sync-lmdb-cache-hit) | sync | 311061.7 | 6.4 | 3113021 | 3113021 | 0 | ingress=2 |

</div>
