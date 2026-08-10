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
