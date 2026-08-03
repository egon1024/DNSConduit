## Evidence

<div class="perf-chart" markdown="1">

### Feature tax — metrics scrape tax (forward_fast)

![Feature tax — metrics scrape tax (forward_fast)](generated/metrics-scrape-ladder-forward-fast.svg)

[Download CSV](generated/metrics-scrape-ladder-forward-fast.csv)

| Posture | Runtime | Achieved QPS | Avg latency (ms) | Sent | Completed | Lost | Workers |
| --- | --- | --- | --- | --- | --- | --- | --- |
| [metrics_off](/performance/scenarios.md#feature-tax-metrics-off-scrape-ladder-forward-fast) | sync | 69443.0 | 2.5 | 698389 | 694725 | 3664 | ingress=2 |
| [metrics_minimal_scrape](/performance/scenarios.md#feature-tax-metrics-minimal-scrape-ladder-forward-fast) | sync | 71784.8 | 4.3 | 721553 | 718176 | 3401 | ingress=2 |
| [metrics_standard_scrape](/performance/scenarios.md#feature-tax-metrics-standard-scrape-ladder-forward-fast) | sync | 66823.7 | 2.9 | 672247 | 668583 | 3664 | ingress=2 |

</div>
