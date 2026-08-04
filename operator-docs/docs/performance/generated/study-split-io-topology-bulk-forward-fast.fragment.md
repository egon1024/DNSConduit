<div class="perf-chart" markdown="1">

### Achieved QPS — split_io topology (forward_fast)

![Achieved QPS — split_io topology (forward_fast)](generated/split-io-topology-bulk-forward-fast.svg)

[Download CSV](generated/split-io-topology-bulk-forward-fast.csv)

| Topology | Runtime | Achieved QPS | Avg latency (ms) | Sent | Completed | Lost | Workers |
| --- | --- | --- | --- | --- | --- | --- | --- |
| [thin](/performance/scenarios.md#scale-split-io-forward-fast) | split_io | 138744.2 | 14.4 | 1389252 | 1389252 | 0 | ingress=2, policy=2, io=2 |
| [heavy](/performance/scenarios.md#scale-split-io-topology-heavy) | split_io | 258908.7 | 7.7 | 2590870 | 2590870 | 0 | ingress=4, policy=4, io=4 |

</div>
