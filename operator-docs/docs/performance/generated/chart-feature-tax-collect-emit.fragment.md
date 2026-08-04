<div class="perf-chart" markdown="1">

### Feature tax — collect vs emit (forward_fast)

![Feature tax — collect vs emit (forward_fast)](generated/feature-tax-collect-emit.svg)

[Download CSV](generated/feature-tax-collect-emit.csv)

| Posture | Achieved QPS | Avg latency (ms) | Sent | Completed | Lost |
| --- | --- | --- | --- | --- | --- |
| [no_collect](/performance/scenarios.md#feature-tax-metrics-no-collect-forward-fast) | 73615.4 | 27.1 | 738135 | 738135 | 0 |
| [collect_only](/performance/scenarios.md#feature-tax-metrics-collect-only-forward-fast) | 70398.5 | 28.3 | 706795 | 706795 | 0 |
| [collect_emit](/performance/scenarios.md#feature-tax-metrics-collect-emit-forward-fast) | 70635.6 | 28.2 | 708920 | 708920 | 0 |

</div>
