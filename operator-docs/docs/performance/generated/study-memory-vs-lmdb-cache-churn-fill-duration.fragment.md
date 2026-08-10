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
