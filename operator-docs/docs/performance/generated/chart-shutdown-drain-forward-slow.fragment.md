<div class="perf-chart" markdown="1">

### Drain duration under forward_slow

![Drain duration under forward_slow](generated/shutdown-drain-forward-slow.svg)

[Download CSV](generated/shutdown-drain-forward-slow.csv)

| Drain policy | Drain duration (ms) | Client failures during stop | QPS | Avg latency (ms) | Sent | Completed |
| --- | --- | --- | --- | --- | --- | --- |
| [drain_complete](/performance/scenarios.md#shutdown-drain-complete-forward-slow) | 113.5 | 299 | 6.5 | 996.6 | 377 | 78 |
| [drain_budgeted](/performance/scenarios.md#shutdown-drain-budgeted-forward-slow) | 113.6 | 200 | 10.0 | 909.2 | 280 | 80 |
| [drain_minimal](/performance/scenarios.md#shutdown-drain-minimal-forward-slow) | 63.7 | 200 | 9.7 | 902.1 | 278 | 78 |

</div>
