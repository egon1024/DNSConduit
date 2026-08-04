<div class="perf-chart" markdown="1">

### Achieved QPS — split_io io_workers series (forward_slow)

![Achieved QPS — split_io io_workers series (forward_slow)](generated/io-vs-ingress-split-forward-slow.svg)

[Download CSV](generated/io-vs-ingress-split-forward-slow.csv)

| I/O workers | Runtime | Achieved QPS | Avg latency (ms) | Sent | Completed | Lost | Workers |
| --- | --- | --- | --- | --- | --- | --- | --- |
| [1](/performance/scenarios.md#scale-split-io-io-1-forward-slow) | split_io | 39149.7 | 51.0 | 1176305 | 1176305 | 0 | ingress=2, policy=2, io=1 |
| [2](/performance/scenarios.md#scale-split-io-forward-slow) | split_io | 39080.2 | 51.1 | 1174342 | 1174342 | 0 | ingress=2, policy=2, io=2 |
| [4](/performance/scenarios.md#scale-split-io-io-4-forward-slow) | split_io | 39132.2 | 51.0 | 1175628 | 1175628 | 0 | ingress=2, policy=2, io=4 |
| [8](/performance/scenarios.md#scale-split-io-io-8-forward-slow) | split_io | 39150.6 | 51.0 | 1176530 | 1176530 | 0 | ingress=2, policy=2, io=8 |

</div>
