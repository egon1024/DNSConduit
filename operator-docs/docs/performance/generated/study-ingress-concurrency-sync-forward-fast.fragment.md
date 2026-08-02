<div class="perf-chart" markdown="1">

### Achieved QPS — sync ingress ladder (forward_fast)

![Achieved QPS — sync ingress ladder (forward_fast)](generated/ingress-concurrency-sync-forward-fast.svg)

[Download CSV](generated/ingress-concurrency-sync-forward-fast.csv)

| Ingress workers | Runtime | Achieved QPS | Avg latency (ms) | Sent | Completed | Lost | Workers |
| --- | --- | --- | --- | --- | --- | --- | --- |
| [1](/performance/scenarios.md#scale-sync-ingress-1-forward-fast) | sync | 38584.0 | 4.3 | 389667 | 386003 | 3664 | ingress=1 |
| [2](/performance/scenarios.md#scale-sync-forward-fast) | sync | 74932.5 | 3.6 | 753275 | 749881 | 3465 | ingress=2 |
| [4](/performance/scenarios.md#scale-sync-ingress-4-forward-fast) | sync | 101956.1 | 2.8 | 1023433 | 1020013 | 3420 | ingress=4 |
| [8](/performance/scenarios.md#scale-sync-ingress-8-forward-fast) | sync | 198042.2 | 2.8 | 1984315 | 1981323 | 2992 | ingress=8 |

</div>
