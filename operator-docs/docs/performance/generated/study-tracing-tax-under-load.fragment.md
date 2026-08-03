## Evidence

<div class="perf-chart" markdown="1">

### Feature tax — tracing off vs on (forward_fast)

![Feature tax — tracing off vs on (forward_fast)](generated/tracing-off-vs-on-forward-fast.svg)

[Download CSV](generated/tracing-off-vs-on-forward-fast.csv)

| Posture | Runtime | Achieved QPS | Avg latency (ms) | Sent | Completed | Lost | Workers |
| --- | --- | --- | --- | --- | --- | --- | --- |
| [metrics_off](/performance/scenarios.md#feature-tax-metrics-off-forward-fast) | sync | 75250.1 | 26.5 | 754909 | 754909 | 0 | ingress=2 |
| [tracing_on](/performance/scenarios.md#feature-tax-tracing-on-forward-fast) | sync | 47100.1 | 42.3 | 473415 | 473415 | 0 | ingress=2 |

</div>
