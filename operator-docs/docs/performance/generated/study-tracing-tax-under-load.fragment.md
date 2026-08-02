## Evidence

<div class="perf-chart" markdown="1">

### Feature tax — tracing off vs on (forward_fast)

![Feature tax — tracing off vs on (forward_fast)](generated/tracing-off-vs-on-forward-fast.svg)

[Download CSV](generated/tracing-off-vs-on-forward-fast.csv)

| Posture | Runtime | Achieved QPS | Avg latency (ms) | Sent | Completed | Lost | Workers |
| --- | --- | --- | --- | --- | --- | --- | --- |
| [metrics_off](/performance/scenarios.md#feature-tax-metrics-off-forward-fast) | sync | 77319.5 | 3.8 | 776925 | 773506 | 3419 | ingress=2 |
| [tracing_on](/performance/scenarios.md#feature-tax-tracing-on-forward-fast) | sync | 49527.8 | 6.3 | 499066 | 495687 | 3379 | ingress=2 |

</div>
