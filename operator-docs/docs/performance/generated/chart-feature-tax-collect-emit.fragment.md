<div class="perf-chart" markdown="1">

### Feature tax — collect vs emit (forward_fast)

![Feature tax — collect vs emit (forward_fast)](generated/feature-tax-collect-emit.svg)

[Download CSV](generated/feature-tax-collect-emit.csv)

| Posture | Achieved QPS | Avg latency (ms) | Sent | Completed | Lost |
| --- | --- | --- | --- | --- | --- |
| [no_collect](/performance/scenarios.md#feature-tax-metrics-no-collect-forward-fast) | 72940.1 | 27.3 | 732015 | 732015 | 0 |
| [collect_only](/performance/scenarios.md#feature-tax-metrics-collect-only-forward-fast) | 69562.3 | 28.7 | 697649 | 697649 | 0 |
| [collect_emit](/performance/scenarios.md#feature-tax-metrics-collect-emit-forward-fast) | 68561.9 | 29.1 | 687918 | 687918 | 0 |

</div>
