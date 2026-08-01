<div class="perf-chart" markdown="1">

### Achieved QPS — sync vs split_io (forward_fast)

![Achieved QPS — sync vs split_io (forward_fast)](generated/scale-sync-vs-split-io-forward-fast.svg)

[Download CSV](generated/scale-sync-vs-split-io-forward-fast.csv)

| Scenario | Runtime | Achieved QPS | Avg latency (ms) | Sent | Completed | Lost | Workers |
| --- | --- | --- | --- | --- | --- | --- | --- |
| [scale-sync-forward-fast](/performance/scenarios.md#scale-sync-forward-fast) | sync | 211448.9 | 1.3 | 2118239 | 2114785 | 3662 | ingress=2 |
| [scale-split-io-forward-fast](/performance/scenarios.md#scale-split-io-forward-fast) | split_io | 364056.4 | 2.2 | 3644026 | 3642237 | 2395 | ingress=2, policy=2, io=2 |

</div>
