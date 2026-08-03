## Evidence

<div class="perf-chart" markdown="1">

### Feature tax — scrape hammer under load (forward_fast)

![Feature tax — scrape hammer under load (forward_fast)](generated/metrics-scrape-hammer-forward-fast.svg)

[Download CSV](generated/metrics-scrape-hammer-forward-fast.csv)

| Posture | Runtime | Achieved QPS | Avg latency (ms) | Sent | Completed | Lost | Workers |
| --- | --- | --- | --- | --- | --- | --- | --- |
| [metrics_off](/performance/scenarios.md#feature-tax-metrics-off-forward-fast) | sync | 75250.1 | 26.5 | 754909 | 754909 | 0 | ingress=2 |
| [metrics_standard_scrape](/performance/scenarios.md#feature-tax-metrics-standard-scrape-forward-fast) | sync | 69858.0 | 28.6 | 700756 | 700756 | 0 | ingress=2 |
| [metrics_standard_scrape_hammer](/performance/scenarios.md#feature-tax-metrics-standard-scrape-hammer-forward-fast) | sync | 60779.5 | 32.8 | 611124 | 611124 | 0 | ingress=2 |

</div>
