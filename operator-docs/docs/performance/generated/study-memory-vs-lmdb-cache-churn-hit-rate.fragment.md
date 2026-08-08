<div class="perf-chart" markdown="1">

### Cache hit rate — sync high-churn (memory vs LMDB)

![Cache hit rate — sync high-churn (memory vs LMDB)](generated/memory-vs-lmdb-cache-churn-hit-rate.svg)

[Download CSV](generated/memory-vs-lmdb-cache-churn-hit-rate.csv)

| Cache Backend | Hit rate (%) | Cache hits | Cache misses | Achieved QPS |
| --- | --- | --- | --- | --- |
| [memory](/performance/scenarios.md#scale-sync-memory-cache-churn) | 49.3 | 454670 | 466895 | 91928.1 |
| [lmdb](/performance/scenarios.md#scale-sync-lmdb-cache-churn) | 26.4 | 2941 | 8195 | 959.3 |

</div>
