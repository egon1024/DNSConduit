<div class="perf-chart" markdown="1">

### Feature tax — collect vs emit (forward_fast)

![Feature tax — collect vs emit (forward_fast)](generated/metrics-collect-vs-emit-forward-fast.svg)

[Download CSV](generated/metrics-collect-vs-emit-forward-fast.csv)

| Posture | Runtime | Achieved QPS | Avg latency (ms) | Sent | Completed | Lost | Workers |
| --- | --- | --- | --- | --- | --- | --- | --- |
| [metrics_no_collect](/performance/scenarios.md#feature-tax-metrics-no-collect-forward-fast) | sync | 72940.1 | 27.3 | 732015 | 732015 | 0 | ingress=2 |
| [metrics_collect_only](/performance/scenarios.md#feature-tax-metrics-collect-only-forward-fast) | sync | 69562.3 | 28.7 | 697649 | 697649 | 0 | ingress=2 |
| [metrics_collect_emit](/performance/scenarios.md#feature-tax-metrics-collect-emit-forward-fast) | sync | 68561.9 | 29.1 | 687918 | 687918 | 0 | ingress=2 |

</div>
