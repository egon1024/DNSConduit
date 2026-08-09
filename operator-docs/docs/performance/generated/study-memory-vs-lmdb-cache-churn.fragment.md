## Evidence

<div class="perf-chart" markdown="1">

### Achieved QPS — sync ingress-8 high-churn (memory vs LMDB sync modes)

![Achieved QPS — sync ingress-8 high-churn (memory vs LMDB sync modes)](generated/memory-vs-lmdb-cache-churn-qps.svg)

[Download CSV](generated/memory-vs-lmdb-cache-churn-qps.csv)

| Lmdb Sync | Runtime | Achieved QPS | Avg latency (ms) | Sent | Completed | Lost | Workers | Hit rate (%) | Cache hits | Cache misses | Fill mean (ms) | Eviction mean (ms) |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| [memory](/performance/scenarios.md#scale-sync-ingress-8-memory-cache-churn) | sync | 268340.7 | 7.4 | 2686723 | 2686723 | 0 | ingress=8 | 49.7 | 1336029 | 1350695 | 0.0005 | 0.0003 |
| [full](/performance/scenarios.md#scale-sync-ingress-8-lmdb-full-cache-churn) | sync | 2957.0 | 653.1 | 31589 | 31589 | 0 | ingress=8 | 42.8 | 13528 | 16630 | 2.6745 | 1.4613 |
| [no_meta](/performance/scenarios.md#scale-sync-ingress-8-lmdb-no_meta-cache-churn) | sync | 4576.2 | 420.9 | 49347 | 49347 | 0 | ingress=8 | 46.5 | 23183 | 25600 | 1.9003 | 1.0559 |
| [none](/performance/scenarios.md#scale-sync-ingress-8-lmdb-none-cache-churn) | sync | 160198.1 | 12.5 | 1605354 | 1605354 | 0 | ingress=8 | 50.1 | 803375 | 801980 | 0.0112 | 0.0059 |

</div>

<div class="perf-chart" markdown="1">

### Cache hit rate — sync ingress-8 high-churn (memory vs LMDB sync modes)

![Cache hit rate — sync ingress-8 high-churn (memory vs LMDB sync modes)](generated/memory-vs-lmdb-cache-churn-hit-rate.svg)

[Download CSV](generated/memory-vs-lmdb-cache-churn-hit-rate.csv)

| Lmdb Sync | Hit rate (%) | Cache hits | Cache misses | Achieved QPS |
| --- | --- | --- | --- | --- |
| [memory](/performance/scenarios.md#scale-sync-ingress-8-memory-cache-churn) | 49.7 | 1336029 | 1350695 | 268340.7 |
| [full](/performance/scenarios.md#scale-sync-ingress-8-lmdb-full-cache-churn) | 42.8 | 13528 | 16630 | 2957.0 |
| [no_meta](/performance/scenarios.md#scale-sync-ingress-8-lmdb-no_meta-cache-churn) | 46.5 | 23183 | 25600 | 4576.2 |
| [none](/performance/scenarios.md#scale-sync-ingress-8-lmdb-none-cache-churn) | 50.1 | 803375 | 801980 | 160198.1 |

</div>

<div class="perf-chart" markdown="1">

### Cache fill mean duration — sync ingress-8 high-churn (memory vs LMDB sync modes)

![Cache fill mean duration — sync ingress-8 high-churn (memory vs LMDB sync modes)](generated/memory-vs-lmdb-cache-churn-fill-duration.svg)

[Download CSV](generated/memory-vs-lmdb-cache-churn-fill-duration.csv)

| Lmdb Sync | Fill mean (ms) | Fill samples | Eviction mean (ms) | Eviction samples | Achieved QPS |
| --- | --- | --- | --- | --- | --- |
| [memory](/performance/scenarios.md#scale-sync-ingress-8-memory-cache-churn) | 0.0005 | 1350695 | 0.0003 | 1342166 | 268340.7 |
| [full](/performance/scenarios.md#scale-sync-ingress-8-lmdb-full-cache-churn) | 2.6745 | 16630 | 1.4613 | 15995 | 2957.0 |
| [no_meta](/performance/scenarios.md#scale-sync-ingress-8-lmdb-no_meta-cache-churn) | 1.9003 | 25600 | 1.0559 | 24576 | 4576.2 |
| [none](/performance/scenarios.md#scale-sync-ingress-8-lmdb-none-cache-churn) | 0.0112 | 801980 | 0.0059 | 799902 | 160198.1 |

</div>
