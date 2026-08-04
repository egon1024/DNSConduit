## Evidence

<div class="perf-chart" markdown="1">

### Feature tax — logging warn vs debug (forward_fast)

![Feature tax — logging warn vs debug (forward_fast)](generated/logging-warn-vs-debug-forward-fast.svg)

[Download CSV](generated/logging-warn-vs-debug-forward-fast.csv)

| Posture | Runtime | Achieved QPS | Avg latency (ms) | Sent | Completed | Lost | Workers |
| --- | --- | --- | --- | --- | --- | --- | --- |
| [logging_warn](/performance/scenarios.md#feature-tax-logging-warn-forward-fast) | sync | 76457.0 | 26.1 | 767234 | 767234 | 0 | ingress=2 |
| [logging_debug](/performance/scenarios.md#feature-tax-logging-debug-forward-fast) | sync | 64029.4 | 31.1 | 642890 | 642890 | 0 | ingress=2 |

</div>
