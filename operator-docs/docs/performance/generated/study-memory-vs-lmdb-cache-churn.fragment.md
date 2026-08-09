## Evidence

<div class="perf-chart" markdown="1">

### Achieved QPS — sync ingress-8 high-churn (memory vs LMDB)

![Achieved QPS — sync ingress-8 high-churn (memory vs LMDB)](generated/memory-vs-lmdb-cache-churn-qps.svg)

[Download CSV](generated/memory-vs-lmdb-cache-churn-qps.csv)

| Cache Backend | Runtime | Achieved QPS | Avg latency (ms) | Sent | Completed | Lost | Workers | Hit rate (%) | Cache hits | Cache misses | Fill mean (ms) | Eviction mean (ms) |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| [memory](/performance/scenarios.md#scale-sync-ingress-8-memory-cache-churn) | sync | 268457.9 | 7.4 | 2687643 | 2687643 | 0 | ingress=8 | 49.7 | 1335770 | 1351874 | 0.0005 | 0.0003 |
| [lmdb](/performance/scenarios.md#scale-sync-ingress-8-lmdb-cache-churn) | sync | 2875.1 | 652.8 | 32244 | 32244 | 0 | ingress=8 | 46.8 | 14368 | 16999 | 2.9683 | 1.6256 |

</div>

<div class="perf-chart" markdown="1">

### Cache hit rate — sync ingress-8 high-churn (memory vs LMDB)

![Cache hit rate — sync ingress-8 high-churn (memory vs LMDB)](generated/memory-vs-lmdb-cache-churn-hit-rate.svg)

[Download CSV](generated/memory-vs-lmdb-cache-churn-hit-rate.csv)

| Cache Backend | Hit rate (%) | Cache hits | Cache misses | Achieved QPS |
| --- | --- | --- | --- | --- |
| [memory](/performance/scenarios.md#scale-sync-ingress-8-memory-cache-churn) | 49.7 | 1335770 | 1351874 | 268457.9 |
| [lmdb](/performance/scenarios.md#scale-sync-ingress-8-lmdb-cache-churn) | 46.8 | 14368 | 16999 | 2875.1 |

</div>

<div class="perf-chart" markdown="1">

### Cache fill mean duration — sync ingress-8 high-churn (memory vs LMDB)

![Cache fill mean duration — sync ingress-8 high-churn (memory vs LMDB)](generated/memory-vs-lmdb-cache-churn-fill-duration.svg)

[Download CSV](generated/memory-vs-lmdb-cache-churn-fill-duration.csv)

| Cache Backend | Fill mean (ms) | Fill samples | Eviction mean (ms) | Eviction samples | Achieved QPS |
| --- | --- | --- | --- | --- | --- |
| [memory](/performance/scenarios.md#scale-sync-ingress-8-memory-cache-churn) | 0.0005 | 1351874 | 0.0003 | 1343747 | 268457.9 |
| [lmdb](/performance/scenarios.md#scale-sync-ingress-8-lmdb-cache-churn) | 2.9683 | 16999 | 1.6256 | 16360 | 2875.1 |

</div>
