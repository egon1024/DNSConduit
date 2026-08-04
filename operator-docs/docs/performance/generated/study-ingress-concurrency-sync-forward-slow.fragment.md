<div class="perf-chart" markdown="1">

### Achieved QPS — sync ingress workers (forward_slow)

![Achieved QPS — sync ingress workers (forward_slow)](generated/ingress-concurrency-sync-forward-slow.svg)

[Download CSV](generated/ingress-concurrency-sync-forward-slow.csv)

| Ingress workers | Runtime | Achieved QPS | Avg latency (ms) | Sent | Completed | Lost | Workers |
| --- | --- | --- | --- | --- | --- | --- | --- |
| [1](/performance/scenarios.md#scale-sync-ingress-1-forward-slow) | sync | 2.9 | 2512.9 | 12094 | 99 | 11995 | ingress=1 |
| [2](/performance/scenarios.md#scale-sync-forward-slow) | sync | 5.7 | 2508.7 | 12188 | 198 | 11990 | ingress=2 |
| [4](/performance/scenarios.md#scale-sync-ingress-4-forward-slow) | sync | 12.4 | 2682.3 | 12393 | 433 | 11960 | ingress=4 |
| [8](/performance/scenarios.md#scale-sync-ingress-8-forward-slow) | sync | 18.1 | 2601.7 | 12586 | 634 | 11952 | ingress=8 |

</div>
