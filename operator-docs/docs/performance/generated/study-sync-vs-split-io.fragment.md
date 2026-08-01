## Evidence

<div class="perf-chart" markdown="1">

### Achieved QPS — sync vs split_io (forward_fast)

![Achieved QPS — sync vs split_io (forward_fast)](generated/sync-vs-split-io-forward-fast.svg)

[Download CSV](generated/sync-vs-split-io-forward-fast.csv)

| Runtime | Achieved QPS | Avg latency (ms) | Sent | Completed | Lost | Workers |
| --- | --- | --- | --- | --- | --- | --- |
| [sync](/performance/scenarios.md#scale-sync-forward-fast) | 211448.9 | 1.3 | 2118239 | 2114785 | 3662 | ingress=2 |
| [split_io](/performance/scenarios.md#scale-split-io-forward-fast) | 364056.4 | 2.2 | 3644026 | 3642237 | 2395 | ingress=2, policy=2, io=2 |

</div>

<div class="perf-chart" markdown="1">

### Achieved QPS — sync vs split_io (forward_slow)

![Achieved QPS — sync vs split_io (forward_slow)](generated/sync-vs-split-io-forward-slow.svg)

[Download CSV](generated/sync-vs-split-io-forward-slow.csv)

| Runtime | Achieved QPS | Avg latency (ms) | Sent | Completed | Lost | Workers |
| --- | --- | --- | --- | --- | --- | --- |
| [sync](/performance/scenarios.md#scale-sync-forward-slow) | 10.2 | 1651.1 | 340 | 153 | 187 | ingress=2 |
| [split_io](/performance/scenarios.md#scale-split-io-forward-slow) | 10.9 | 3320.2 | 298 | 161 | 137 | ingress=2, policy=2, io=2 |

</div>
