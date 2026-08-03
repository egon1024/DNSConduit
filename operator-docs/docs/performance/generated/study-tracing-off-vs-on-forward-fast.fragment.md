<div class="perf-chart" markdown="1">

### Feature tax — tracing off vs on (forward_fast)

![Feature tax — tracing off vs on (forward_fast)](generated/tracing-off-vs-on-forward-fast.svg)

[Download CSV](generated/tracing-off-vs-on-forward-fast.csv)

| Posture | Runtime | Achieved QPS | Avg latency (ms) | Sent | Completed | Lost | Workers |
| --- | --- | --- | --- | --- | --- | --- | --- |
| [metrics_off](/performance/scenarios.md#feature-tax-metrics-off-forward-fast) | sync | 75462.4 | 4.0 | 758529 | 755155 | 3400 | ingress=2 |
| [tracing_on](/performance/scenarios.md#feature-tax-tracing-on-forward-fast) | sync | 45497.7 | 3.9 | 458959 | 455295 | 3664 | ingress=2 |

</div>
