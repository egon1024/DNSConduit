<div class="perf-chart" markdown="1">

### Achieved QPS — sync vs split_io (forward_fast)

![Achieved QPS — sync vs split_io (forward_fast)](generated/sync-vs-split-io-forward-fast.svg)

[Download CSV](generated/sync-vs-split-io-forward-fast.csv)

| Runtime | Achieved QPS | Avg latency (ms) | Sent | Completed | Lost | Workers |
| --- | --- | --- | --- | --- | --- | --- |
| [sync](/performance/scenarios.md#scale-sync-forward-fast) | 74932.5 | 3.6 | 753275 | 749881 | 3465 | ingress=2 |
| [split_io](/performance/scenarios.md#scale-split-io-forward-fast) | 140686.6 | 1.4 | 1424907 | 1421028 | 3879 | ingress=2, policy=2, io=2 |

</div>
