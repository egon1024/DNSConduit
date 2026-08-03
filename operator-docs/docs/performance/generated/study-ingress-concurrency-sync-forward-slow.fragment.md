<div class="perf-chart" markdown="1">

### Achieved QPS — sync ingress workers (forward_slow)

![Achieved QPS — sync ingress workers (forward_slow)](generated/ingress-concurrency-sync-forward-slow.svg)

[Download CSV](generated/ingress-concurrency-sync-forward-slow.csv)

| Ingress workers | Runtime | Achieved QPS | Avg latency (ms) | Sent | Completed | Lost | Workers |
| --- | --- | --- | --- | --- | --- | --- | --- |
| [1](/performance/scenarios.md#scale-sync-ingress-1-forward-slow) | sync | 2.9 | 2514.4 | 12093 | 99 | 11994 | ingress=1 |
| [2](/performance/scenarios.md#scale-sync-forward-slow) | sync | 5.7 | 2507.6 | 12188 | 198 | 11990 | ingress=2 |
| [4](/performance/scenarios.md#scale-sync-ingress-4-forward-slow) | sync | 11.4 | 2513.0 | 12377 | 398 | 11979 | ingress=4 |
| [8](/performance/scenarios.md#scale-sync-ingress-8-forward-slow) | sync | 22.5 | 2640.7 | 12699 | 786 | 11929 | ingress=8 |

</div>
