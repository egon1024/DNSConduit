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
