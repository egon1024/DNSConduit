<div class="perf-chart" markdown="1">

### Achieved QPS — sync vs split_io (forward_slow)

![Achieved QPS — sync vs split_io (forward_slow)](generated/sync-vs-split-io-forward-slow.svg)

[Download CSV](generated/sync-vs-split-io-forward-slow.csv)

| Runtime | Achieved QPS | Avg latency (ms) | Sent | Completed | Lost | Workers |
| --- | --- | --- | --- | --- | --- | --- |
| [sync](/performance/scenarios.md#scale-sync-forward-slow) | 10.2 | 1651.1 | 340 | 153 | 187 | ingress=2 |
| [split_io](/performance/scenarios.md#scale-split-io-forward-slow) | 10.9 | 3320.2 | 298 | 161 | 137 | ingress=2, policy=2, io=2 |

</div>
