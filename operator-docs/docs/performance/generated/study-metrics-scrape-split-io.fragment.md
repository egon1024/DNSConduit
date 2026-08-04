## Evidence

<div class="perf-chart" markdown="1">

### Feature tax — metrics scrape under split_io (forward_fast)

![Feature tax — metrics scrape under split_io (forward_fast)](generated/metrics-scrape-split-io-forward-fast.svg)

[Download CSV](generated/metrics-scrape-split-io-forward-fast.csv)

| Posture | Runtime | Achieved QPS | Avg latency (ms) | Sent | Completed | Lost | Workers |
| --- | --- | --- | --- | --- | --- | --- | --- |
| [metrics_off](/performance/scenarios.md#feature-tax-metrics-off-split-io-forward-fast) | split_io | 146802.2 | 13.6 | 1469986 | 1469986 | 0 | ingress=2, policy=2, io=2 |
| [metrics_standard_scrape](/performance/scenarios.md#feature-tax-metrics-standard-scrape-split-io-forward-fast) | split_io | 139823.3 | 14.3 | 1400096 | 1400096 | 0 | ingress=2, policy=2, io=2 |

</div>
