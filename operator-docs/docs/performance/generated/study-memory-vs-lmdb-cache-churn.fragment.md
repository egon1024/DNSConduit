## Evidence

<div class="perf-chart" markdown="1">

### Achieved QPS — sync high-churn cache (memory vs LMDB)

![Achieved QPS — sync high-churn cache (memory vs LMDB)](generated/memory-vs-lmdb-cache-churn-qps.svg)

[Download CSV](generated/memory-vs-lmdb-cache-churn-qps.csv)

| Cache Backend | Runtime | Achieved QPS | Avg latency (ms) | Sent | Completed | Lost | Workers | Hit rate (%) | Cache hits | Cache misses |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| [memory](/performance/scenarios.md#scale-sync-memory-cache-churn) | sync | 91928.1 | 21.7 | 921564 | 921564 | 0 | ingress=2 | 49.3 | 454670 | 466895 |
| [lmdb](/performance/scenarios.md#scale-sync-lmdb-cache-churn) | sync | 959.3 | 1937.8 | 11135 | 11135 | 0 | ingress=2 | 26.4 | 2941 | 8195 |

</div>

<div class="perf-chart" markdown="1">

### Cache hit rate — sync high-churn (memory vs LMDB)

![Cache hit rate — sync high-churn (memory vs LMDB)](generated/memory-vs-lmdb-cache-churn-hit-rate.svg)

[Download CSV](generated/memory-vs-lmdb-cache-churn-hit-rate.csv)

| Cache Backend | Hit rate (%) | Cache hits | Cache misses | Achieved QPS |
| --- | --- | --- | --- | --- |
| [memory](/performance/scenarios.md#scale-sync-memory-cache-churn) | 49.3 | 454670 | 466895 | 91928.1 |
| [lmdb](/performance/scenarios.md#scale-sync-lmdb-cache-churn) | 26.4 | 2941 | 8195 | 959.3 |

</div>
