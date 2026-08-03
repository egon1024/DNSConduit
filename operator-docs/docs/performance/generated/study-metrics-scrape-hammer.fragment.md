## Evidence

<div class="perf-chart" markdown="1">

### Feature tax — scrape hammer under load (forward_fast)

![Feature tax — scrape hammer under load (forward_fast)](generated/metrics-scrape-hammer-forward-fast.svg)

[Download CSV](generated/metrics-scrape-hammer-forward-fast.csv)

| Posture | Runtime | Achieved QPS | Avg latency (ms) | Sent | Completed | Lost | Workers |
| --- | --- | --- | --- | --- | --- | --- | --- |
| [metrics_off](/performance/scenarios.md#feature-tax-metrics-off-forward-fast) | sync | 75462.4 | 4.0 | 758529 | 755155 | 3400 | ingress=2 |
| [metrics_standard_scrape](/performance/scenarios.md#feature-tax-metrics-standard-scrape-forward-fast) | sync | 69628.0 | 4.5 | 700042 | 696579 | 3377 | ingress=2 |
| [metrics_standard_scrape_hammer](/performance/scenarios.md#feature-tax-metrics-standard-scrape-hammer-forward-fast) | sync | 68340.8 | 4.5 | 687351 | 683962 | 3389 | ingress=2 |

</div>
