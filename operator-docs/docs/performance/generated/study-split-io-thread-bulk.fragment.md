## Evidence

<div class="perf-chart" markdown="1">

### Achieved QPS — split_io topology (forward_fast)

![Achieved QPS — split_io topology (forward_fast)](generated/split-io-topology-bulk-forward-fast.svg)

[Download CSV](generated/split-io-topology-bulk-forward-fast.csv)

| Topology | Runtime | Achieved QPS | Avg latency (ms) | Sent | Completed | Lost | Workers |
| --- | --- | --- | --- | --- | --- | --- | --- |
| [thin](/performance/scenarios.md#scale-split-io-forward-fast) | split_io | 364056.4 | 2.2 | 3644026 | 3642237 | 2395 | ingress=2, policy=2, io=2 |
| [heavy](/performance/scenarios.md#scale-split-io-topology-heavy) | split_io | 601897.6 | 2.7 | 6021233 | 6020309 | 611 | ingress=4, policy=4, io=4 |

</div>
