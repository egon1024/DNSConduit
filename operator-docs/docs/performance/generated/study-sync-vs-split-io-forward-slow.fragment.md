<div class="perf-chart" markdown="1">

### Achieved QPS — sync vs split_io (forward_slow)

![Achieved QPS — sync vs split_io (forward_slow)](generated/sync-vs-split-io-forward-slow.svg)

[Download CSV](generated/sync-vs-split-io-forward-slow.csv)

| Runtime | Achieved QPS | Avg latency (ms) | Sent | Completed | Lost | Workers |
| --- | --- | --- | --- | --- | --- | --- |
| [sync](/performance/scenarios.md#scale-sync-forward-slow) | 5.7 | 2508.7 | 12188 | 198 | 11990 | ingress=2 |
| [split_io](/performance/scenarios.md#scale-split-io-forward-slow) | 39080.2 | 51.1 | 1174342 | 1174342 | 0 | ingress=2, policy=2, io=2 |

</div>
