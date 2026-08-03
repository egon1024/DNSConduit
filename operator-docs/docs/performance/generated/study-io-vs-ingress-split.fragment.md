## Evidence

<div class="perf-chart" markdown="1">

### Achieved QPS — split_io io_workers series (forward_slow)

![Achieved QPS — split_io io_workers series (forward_slow)](generated/io-vs-ingress-split-forward-slow.svg)

[Download CSV](generated/io-vs-ingress-split-forward-slow.csv)

| I/O workers | Runtime | Achieved QPS | Avg latency (ms) | Sent | Completed | Lost | Workers |
| --- | --- | --- | --- | --- | --- | --- | --- |
| [1](/performance/scenarios.md#scale-split-io-io-1-forward-slow) | split_io | 38839.5 | 51.3 | 1168707 | 1168707 | 0 | ingress=2, policy=2, io=1 |
| [2](/performance/scenarios.md#scale-split-io-forward-slow) | split_io | 38736.3 | 51.3 | 1167745 | 1167745 | 0 | ingress=2, policy=2, io=2 |
| [4](/performance/scenarios.md#scale-split-io-io-4-forward-slow) | split_io | 38855.5 | 51.2 | 1171188 | 1171188 | 0 | ingress=2, policy=2, io=4 |
| [8](/performance/scenarios.md#scale-split-io-io-8-forward-slow) | split_io | 38642.6 | 51.5 | 1165050 | 1165050 | 0 | ingress=2, policy=2, io=8 |

</div>
