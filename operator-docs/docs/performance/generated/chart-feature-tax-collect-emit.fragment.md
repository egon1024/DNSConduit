<div class="perf-chart" markdown="1">

### Feature tax — collect vs emit (forward_fast)

![Feature tax — collect vs emit (forward_fast)](generated/feature-tax-collect-emit.svg)

[Download CSV](generated/feature-tax-collect-emit.csv)

| Posture | Achieved QPS | Avg latency (ms) | Sent | Completed | Lost |
| --- | --- | --- | --- | --- | --- |
| [no_collect](/performance/scenarios.md#feature-tax-metrics-no-collect-forward-fast) | 128663.3 | 2.1 | 1290379 | 1286917 | 3462 |
| [collect_only](/performance/scenarios.md#feature-tax-metrics-collect-only-forward-fast) | 121086.6 | 1.6 | 1214611 | 1211152 | 3484 |
| [collect_emit](/performance/scenarios.md#feature-tax-metrics-collect-emit-forward-fast) | 117515.8 | 2.6 | 1178865 | 1175436 | 3429 |

</div>
