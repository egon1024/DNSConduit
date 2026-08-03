<div class="perf-chart" markdown="1">

### Achieved QPS — sync ingress workers (forward_fast)

![Achieved QPS — sync ingress workers (forward_fast)](generated/ingress-concurrency-sync-forward-fast.svg)

[Download CSV](generated/ingress-concurrency-sync-forward-fast.csv)

| Ingress workers | Runtime | Achieved QPS | Avg latency (ms) | Sent | Completed | Lost | Workers |
| --- | --- | --- | --- | --- | --- | --- | --- |
| [1](/performance/scenarios.md#scale-sync-ingress-1-forward-fast) | sync | 38612.6 | 4.3 | 389959 | 386295 | 3664 | ingress=1 |
| [2](/performance/scenarios.md#scale-sync-forward-fast) | sync | 75601.1 | 3.8 | 759747 | 756344 | 3443 | ingress=2 |
| [4](/performance/scenarios.md#scale-sync-ingress-4-forward-fast) | sync | 134945.7 | 2.8 | 1353302 | 1350035 | 3267 | ingress=4 |
| [8](/performance/scenarios.md#scale-sync-ingress-8-forward-fast) | sync | 205558.9 | 3.4 | 2059455 | 2056472 | 2862 | ingress=8 |

</div>
