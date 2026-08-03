## Evidence

<div class="perf-chart" markdown="1">

### Feature tax — metrics scrape under split_io (forward_fast)

![Feature tax — metrics scrape under split_io (forward_fast)](generated/metrics-scrape-split-io-forward-fast.svg)

[Download CSV](generated/metrics-scrape-split-io-forward-fast.csv)

| Posture | Runtime | Achieved QPS | Avg latency (ms) | Sent | Completed | Lost | Workers |
| --- | --- | --- | --- | --- | --- | --- | --- |
| [metrics_off](/performance/scenarios.md#feature-tax-metrics-off-split-io-forward-fast) | split_io | 130924.8 | 1.4 | 1318112 | 1314408 | 3870 | ingress=2, policy=2, io=2 |
| [metrics_standard_scrape](/performance/scenarios.md#feature-tax-metrics-standard-scrape-split-io-forward-fast) | split_io | 134804.5 | 1.8 | 1368825 | 1365195 | 3702 | ingress=2, policy=2, io=2 |

</div>
