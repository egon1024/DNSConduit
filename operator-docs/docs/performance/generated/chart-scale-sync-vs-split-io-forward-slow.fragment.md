<div class="perf-chart" markdown="1">

### Achieved QPS — sync vs split_io (forward_slow)

![Achieved QPS — sync vs split_io (forward_slow)](generated/scale-sync-vs-split-io-forward-slow.svg)

[Download CSV](generated/scale-sync-vs-split-io-forward-slow.csv)

| Scenario | Runtime | Achieved QPS | Avg latency (ms) | Sent | Completed | Lost | Workers |
| --- | --- | --- | --- | --- | --- | --- | --- |
| [scale-sync-forward-slow](/performance/scenarios.md#scale-sync-forward-slow) | sync | 10.2 | 1651.1 | 340 | 153 | 187 | ingress=2 |
| [scale-split-io-forward-slow](/performance/scenarios.md#scale-split-io-forward-slow) | split_io | 10.9 | 3320.2 | 298 | 161 | 137 | ingress=2, policy=2, io=2 |

</div>
