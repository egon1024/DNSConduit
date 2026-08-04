## Evidence

<div class="perf-chart" markdown="1">

### Feature tax — metrics scrape tax (forward_fast)

![Feature tax — metrics scrape tax (forward_fast)](generated/metrics-scrape-ladder-forward-fast.svg)

[Download CSV](generated/metrics-scrape-ladder-forward-fast.csv)

| Posture | Runtime | Achieved QPS | Avg latency (ms) | Sent | Completed | Lost | Workers |
| --- | --- | --- | --- | --- | --- | --- | --- |
| [metrics_off](/performance/scenarios.md#feature-tax-metrics-off-scrape-ladder-forward-fast) | sync | 76124.1 | 26.2 | 763843 | 763843 | 0 | ingress=2 |
| [metrics_minimal_scrape](/performance/scenarios.md#feature-tax-metrics-minimal-scrape-ladder-forward-fast) | sync | 73038.1 | 27.3 | 732337 | 732337 | 0 | ingress=2 |
| [metrics_standard_scrape](/performance/scenarios.md#feature-tax-metrics-standard-scrape-ladder-forward-fast) | sync | 70157.8 | 28.4 | 703550 | 703550 | 0 | ingress=2 |

</div>
