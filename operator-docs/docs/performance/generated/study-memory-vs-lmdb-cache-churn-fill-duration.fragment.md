<div class="perf-chart" markdown="1">

### Cache fill mean duration — sync ingress-8 high-churn (memory vs LMDB)

![Cache fill mean duration — sync ingress-8 high-churn (memory vs LMDB)](generated/memory-vs-lmdb-cache-churn-fill-duration.svg)

[Download CSV](generated/memory-vs-lmdb-cache-churn-fill-duration.csv)

| Cache Backend | Fill mean (ms) | Fill samples | Eviction mean (ms) | Eviction samples | Achieved QPS |
| --- | --- | --- | --- | --- | --- |
| [memory](/performance/scenarios.md#scale-sync-ingress-8-memory-cache-churn) | 0.0005 | 1351874 | 0.0003 | 1343747 | 268457.9 |
| [lmdb](/performance/scenarios.md#scale-sync-ingress-8-lmdb-cache-churn) | 2.9683 | 16999 | 1.6256 | 16360 | 2875.1 |

</div>
