## Evidence

<div class="perf-chart" markdown="1">

### Feature tax — metrics scrape ladder (forward_fast)

![Feature tax — metrics scrape ladder (forward_fast)](generated/metrics-scrape-ladder-forward-fast.svg)

[Download CSV](generated/metrics-scrape-ladder-forward-fast.csv)

| Posture | Runtime | Achieved QPS | Avg latency (ms) | Sent | Completed | Lost | Workers |
| --- | --- | --- | --- | --- | --- | --- | --- |
| [metrics_off](/performance/scenarios.md#feature-tax-metrics-off-scrape-ladder-forward-fast) | sync | 138672.0 | 2.2 | 1390396 | 1387024 | 3377 | ingress=2 |
| [metrics_minimal_scrape](/performance/scenarios.md#feature-tax-metrics-minimal-scrape-ladder-forward-fast) | sync | 131434.7 | 2.1 | 1318106 | 1314639 | 3467 | ingress=2 |
| [metrics_standard_scrape](/performance/scenarios.md#feature-tax-metrics-standard-scrape-ladder-forward-fast) | sync | 121094.1 | 1.5 | 1214666 | 1211244 | 3508 | ingress=2 |

</div>
