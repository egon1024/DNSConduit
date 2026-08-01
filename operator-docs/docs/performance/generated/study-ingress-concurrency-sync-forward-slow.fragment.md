<div class="perf-chart" markdown="1">

### Achieved QPS — sync ingress ladder (forward_slow)

![Achieved QPS — sync ingress ladder (forward_slow)](generated/ingress-concurrency-sync-forward-slow.svg)

[Download CSV](generated/ingress-concurrency-sync-forward-slow.csv)

| Ingress workers | Runtime | Achieved QPS | Avg latency (ms) | Sent | Completed | Lost | Workers |
| --- | --- | --- | --- | --- | --- | --- | --- |
| [1](/performance/scenarios.md#scale-sync-ingress-1-forward-slow) | sync | 6.7 | 2521.0 | 299 | 100 | 199 | ingress=1 |
| [2](/performance/scenarios.md#scale-sync-forward-slow) | sync | 10.2 | 1651.1 | 340 | 153 | 187 | ingress=2 |
| [4](/performance/scenarios.md#scale-sync-ingress-4-forward-slow) | sync | 6.7 | 2518.5 | 299 | 100 | 199 | ingress=4 |
| [8](/performance/scenarios.md#scale-sync-ingress-8-forward-slow) | sync | 6.7 | 2518.6 | 299 | 100 | 199 | ingress=8 |

</div>
