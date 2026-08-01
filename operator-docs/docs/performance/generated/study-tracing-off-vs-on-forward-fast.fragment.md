<div class="perf-chart" markdown="1">

### Feature tax — tracing off vs on (forward_fast)

![Feature tax — tracing off vs on (forward_fast)](generated/tracing-off-vs-on-forward-fast.svg)

[Download CSV](generated/tracing-off-vs-on-forward-fast.csv)

| Posture | Runtime | Achieved QPS | Avg latency (ms) | Sent | Completed | Lost | Workers |
| --- | --- | --- | --- | --- | --- | --- | --- |
| [metrics_off](/performance/scenarios.md#feature-tax-metrics-off-forward-fast) | sync | 138760.8 | 2.0 | 1391423 | 1387890 | 3449 | ingress=2 |
| [tracing_on](/performance/scenarios.md#feature-tax-tracing-on-forward-fast) | sync | 71846.1 | 3.8 | 722221 | 718827 | 3455 | ingress=2 |

</div>
