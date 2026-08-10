## Evidence

<div class="perf-chart" markdown="1">

### Achieved QPS — sync ingress-8 high-churn (memory vs LMDB sync modes)

![Achieved QPS — sync ingress-8 high-churn (memory vs LMDB sync modes)](generated/memory-vs-lmdb-cache-churn-qps.svg)

[Download CSV](generated/memory-vs-lmdb-cache-churn-qps.csv)

| Lmdb Sync | Runtime | Achieved QPS | Avg latency (ms) | Sent | Completed | Lost | Workers | Hit rate (%) | Cache hits | Cache misses | Fill mean (ms) | Eviction mean (ms) |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| [memory](/performance/scenarios.md#scale-sync-ingress-8-memory-cache-churn) | sync | 232253.7 | 8.6 | 2325790 | 2325790 | 0 | ingress=8 | 49.7 | 1154131 | 1171660 | 0.0005 | 0.0003 |
| [full](/performance/scenarios.md#scale-sync-ingress-8-lmdb-full-cache-churn) | sync | 2767.2 | 698.4 | 29235 | 29235 | 0 | ingress=8 | 42.1 | 12317 | 16919 | 2.8751 | 1.6619 |
| [no_meta](/performance/scenarios.md#scale-sync-ingress-8-lmdb-no_meta-cache-churn) | sync | 4379.2 | 442.1 | 46180 | 46180 | 0 | ingress=8 | 45.0 | 20520 | 25094 | 1.9188 | 1.0409 |
| [periodic](/performance/scenarios.md#scale-sync-ingress-8-lmdb-periodic-cache-churn) | sync | 191677.2 | 10.4 | 1920116 | 1920116 | 0 | ingress=8 | 50.2 | 964032 | 956085 | 0.0129 | 0.0068 |
| [none](/performance/scenarios.md#scale-sync-ingress-8-lmdb-none-cache-churn) | sync | 176805.2 | 11.3 | 1771208 | 1771208 | 0 | ingress=8 | 50.0 | 886097 | 885112 | 0.0124 | 0.0065 |

</div>

<div class="perf-chart" markdown="1">

### Cache hit rate — sync ingress-8 high-churn (memory vs LMDB sync modes)

![Cache hit rate — sync ingress-8 high-churn (memory vs LMDB sync modes)](generated/memory-vs-lmdb-cache-churn-hit-rate.svg)

[Download CSV](generated/memory-vs-lmdb-cache-churn-hit-rate.csv)

| Lmdb Sync | Hit rate (%) | Cache hits | Cache misses | Achieved QPS |
| --- | --- | --- | --- | --- |
| [memory](/performance/scenarios.md#scale-sync-ingress-8-memory-cache-churn) | 49.7 | 1154131 | 1171660 | 232253.7 |
| [full](/performance/scenarios.md#scale-sync-ingress-8-lmdb-full-cache-churn) | 42.1 | 12317 | 16919 | 2767.2 |
| [no_meta](/performance/scenarios.md#scale-sync-ingress-8-lmdb-no_meta-cache-churn) | 45.0 | 20520 | 25094 | 4379.2 |
| [periodic](/performance/scenarios.md#scale-sync-ingress-8-lmdb-periodic-cache-churn) | 50.2 | 964032 | 956085 | 191677.2 |
| [none](/performance/scenarios.md#scale-sync-ingress-8-lmdb-none-cache-churn) | 50.0 | 886097 | 885112 | 176805.2 |

</div>

<div class="perf-chart" markdown="1">

### Cache fill mean duration — sync ingress-8 high-churn (memory vs LMDB sync modes)

![Cache fill mean duration — sync ingress-8 high-churn (memory vs LMDB sync modes)](generated/memory-vs-lmdb-cache-churn-fill-duration.svg)

[Download CSV](generated/memory-vs-lmdb-cache-churn-fill-duration.csv)

| Lmdb Sync | Fill mean (ms) | Fill samples | Eviction mean (ms) | Eviction samples | Achieved QPS |
| --- | --- | --- | --- | --- | --- |
| [memory](/performance/scenarios.md#scale-sync-ingress-8-memory-cache-churn) | 0.0005 | 1171660 | 0.0003 | 1164824 | 232253.7 |
| [full](/performance/scenarios.md#scale-sync-ingress-8-lmdb-full-cache-churn) | 2.8751 | 16919 | 1.6619 | 14884 | 2767.2 |
| [no_meta](/performance/scenarios.md#scale-sync-ingress-8-lmdb-no_meta-cache-churn) | 1.9188 | 25094 | 1.0409 | 23629 | 4379.2 |
| [periodic](/performance/scenarios.md#scale-sync-ingress-8-lmdb-periodic-cache-churn) | 0.0129 | 956085 | 0.0068 | 954046 | 191677.2 |
| [none](/performance/scenarios.md#scale-sync-ingress-8-lmdb-none-cache-churn) | 0.0124 | 885112 | 0.0065 | 883086 | 176805.2 |

</div>
