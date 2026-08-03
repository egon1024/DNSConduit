## Evidence

<div class="perf-chart" markdown="1">

### Achieved QPS — sync vs split_io (forward_fast)

![Achieved QPS — sync vs split_io (forward_fast)](generated/sync-vs-split-io-forward-fast.svg)

[Download CSV](generated/sync-vs-split-io-forward-fast.csv)

| Runtime | Achieved QPS | Avg latency (ms) | Sent | Completed | Lost | Workers |
| --- | --- | --- | --- | --- | --- | --- |
| [sync](/performance/scenarios.md#scale-sync-forward-fast) | 73594.4 | 27.1 | 738415 | 738415 | 0 | ingress=2 |
| [split_io](/performance/scenarios.md#scale-split-io-forward-fast) | 141401.1 | 14.0 | 1430029 | 1430029 | 0 | ingress=2, policy=2, io=2 |

</div>

<div class="perf-chart" markdown="1">

### Achieved QPS — sync vs split_io (forward_slow)

![Achieved QPS — sync vs split_io (forward_slow)](generated/sync-vs-split-io-forward-slow.svg)

[Download CSV](generated/sync-vs-split-io-forward-slow.csv)

| Runtime | Achieved QPS | Avg latency (ms) | Sent | Completed | Lost | Workers |
| --- | --- | --- | --- | --- | --- | --- |
| [sync](/performance/scenarios.md#scale-sync-forward-slow) | 5.7 | 2509.2 | 12188 | 198 | 11990 | ingress=2 |
| [split_io](/performance/scenarios.md#scale-split-io-forward-slow) | 38736.3 | 51.3 | 1167745 | 1167745 | 0 | ingress=2, policy=2, io=2 |

</div>
