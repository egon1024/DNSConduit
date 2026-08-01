<div class="perf-chart" markdown="1">

### Achieved QPS — split_io io_workers ladder (forward_slow)

![Achieved QPS — split_io io_workers ladder (forward_slow)](generated/io-vs-ingress-split-forward-slow.svg)

[Download CSV](generated/io-vs-ingress-split-forward-slow.csv)

| I/O workers | Runtime | Achieved QPS | Avg latency (ms) | Sent | Completed | Lost | Workers |
| --- | --- | --- | --- | --- | --- | --- | --- |
| [1](/performance/scenarios.md#scale-split-io-io-1-forward-slow) | split_io | 10.5 | 2751.1 | 297 | 145 | 151 | ingress=2, policy=2, io=1 |
| [2](/performance/scenarios.md#scale-split-io-forward-slow) | split_io | 10.9 | 3320.2 | 298 | 161 | 137 | ingress=2, policy=2, io=2 |
| [4](/performance/scenarios.md#scale-split-io-io-4-forward-slow) | split_io | 9.0 | 3152.0 | 299 | 134 | 164 | ingress=2, policy=2, io=4 |
| [8](/performance/scenarios.md#scale-split-io-io-8-forward-slow) | split_io | 8.9 | 3148.5 | 299 | 133 | 165 | ingress=2, policy=2, io=8 |

</div>
