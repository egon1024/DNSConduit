<div class="perf-chart" markdown="1">

### Achieved QPS — sync ingress workers (forward_fast)

![Achieved QPS — sync ingress workers (forward_fast)](generated/ingress-concurrency-sync-forward-fast.svg)

[Download CSV](generated/ingress-concurrency-sync-forward-fast.csv)

| Ingress workers | Runtime | Achieved QPS | Avg latency (ms) | Sent | Completed | Lost | Workers |
| --- | --- | --- | --- | --- | --- | --- | --- |
| [1](/performance/scenarios.md#scale-sync-ingress-1-forward-fast) | sync | 38267.6 | 52.1 | 384651 | 384651 | 0 | ingress=1 |
| [2](/performance/scenarios.md#scale-sync-forward-fast) | sync | 73594.4 | 27.1 | 738415 | 738415 | 0 | ingress=2 |
| [4](/performance/scenarios.md#scale-sync-ingress-4-forward-fast) | sync | 146451.1 | 13.6 | 1468917 | 1468917 | 0 | ingress=4 |
| [8](/performance/scenarios.md#scale-sync-ingress-8-forward-fast) | sync | 239549.0 | 8.3 | 2400186 | 2400186 | 0 | ingress=8 |

</div>
