<div class="perf-chart" markdown="1">

### Achieved QPS — sync vs split_io (forward_fast)

![Achieved QPS — sync vs split_io (forward_fast)](generated/scale-sync-vs-split-io-forward-fast.svg)

[Download CSV](generated/scale-sync-vs-split-io-forward-fast.csv)

| Scenario | Runtime | Achieved QPS | Avg latency (ms) | Sent | Completed | Lost | Workers |
| --- | --- | --- | --- | --- | --- | --- | --- |
| [scale-sync-forward-fast](/performance/scenarios.md#scale-sync-forward-fast) | sync | 76269.9 | 26.1 | 765379 | 765379 | 0 | ingress=2 |
| [scale-split-io-forward-fast](/performance/scenarios.md#scale-split-io-forward-fast) | split_io | 138744.2 | 14.4 | 1389252 | 1389252 | 0 | ingress=2, policy=2, io=2 |

</div>
