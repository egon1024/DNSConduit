<div class="perf-chart" markdown="1">

### Drain duration under forward_slow

![Drain duration under forward_slow](generated/shutdown-drain-forward-slow.svg)

[Download CSV](generated/shutdown-drain-forward-slow.csv)

| Drain policy | Drain duration (ms) | Client failures during stop | QPS | Avg latency (ms) | Sent | Completed |
| --- | --- | --- | --- | --- | --- | --- |
| [drain_complete](/performance/scenarios.md#shutdown-drain-complete-forward-slow) | 113.9 | 300 | 3.2 | 949.9 | 339 | 39 |
| [drain_budgeted](/performance/scenarios.md#shutdown-drain-budgeted-forward-slow) | 163.6 | 200 | 5.0 | 1022.3 | 240 | 40 |
| [drain_minimal](/performance/scenarios.md#shutdown-drain-minimal-forward-slow) | 63.8 | 200 | 4.9 | 996.3 | 239 | 39 |

</div>
