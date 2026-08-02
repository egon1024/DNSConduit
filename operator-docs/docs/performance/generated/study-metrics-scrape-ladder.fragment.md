## Evidence

<div class="perf-chart" markdown="1">

### Feature tax — metrics scrape ladder (forward_fast)

![Feature tax — metrics scrape ladder (forward_fast)](generated/metrics-scrape-ladder-forward-fast.svg)

[Download CSV](generated/metrics-scrape-ladder-forward-fast.csv)

| Posture | Runtime | Achieved QPS | Avg latency (ms) | Sent | Completed | Lost | Workers |
| --- | --- | --- | --- | --- | --- | --- | --- |
| [metrics_off](/performance/scenarios.md#feature-tax-metrics-off-scrape-ladder-forward-fast) | sync | 75321.6 | 3.9 | 757077 | 753665 | 3410 | ingress=2 |
| [metrics_minimal_scrape](/performance/scenarios.md#feature-tax-metrics-minimal-scrape-ladder-forward-fast) | sync | 73415.2 | 4.3 | 737853 | 734479 | 3393 | ingress=2 |
| [metrics_standard_scrape](/performance/scenarios.md#feature-tax-metrics-standard-scrape-ladder-forward-fast) | sync | 66697.3 | 2.6 | 670934 | 667270 | 3664 | ingress=2 |

</div>
