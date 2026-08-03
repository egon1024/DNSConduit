<div class="perf-chart" markdown="1">

### Feature tax — collect vs emit (forward_fast)

![Feature tax — collect vs emit (forward_fast)](generated/feature-tax-collect-emit.svg)

[Download CSV](generated/feature-tax-collect-emit.csv)

| Posture | Achieved QPS | Avg latency (ms) | Sent | Completed | Lost |
| --- | --- | --- | --- | --- | --- |
| [no_collect](/performance/scenarios.md#feature-tax-metrics-no-collect-forward-fast) | 67772.4 | 2.7 | 681748 | 678084 | 3664 |
| [collect_only](/performance/scenarios.md#feature-tax-metrics-collect-only-forward-fast) | 69703.6 | 4.2 | 700769 | 697334 | 3418 |
| [collect_emit](/performance/scenarios.md#feature-tax-metrics-collect-emit-forward-fast) | 63894.6 | 2.8 | 642896 | 639232 | 3664 |

</div>
