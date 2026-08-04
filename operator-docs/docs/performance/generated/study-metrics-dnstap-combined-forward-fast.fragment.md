<div class="perf-chart" markdown="1">

### Feature tax — metrics and dnstap combined (forward_fast)

![Feature tax — metrics and dnstap combined (forward_fast)](generated/metrics-dnstap-combined-forward-fast.svg)

[Download CSV](generated/metrics-dnstap-combined-forward-fast.csv)

| Posture | Runtime | Achieved QPS | Avg latency (ms) | Sent | Completed | Lost | Workers |
| --- | --- | --- | --- | --- | --- | --- | --- |
| [metrics_off](/performance/scenarios.md#feature-tax-metrics-off-forward-fast) | sync | 75940.7 | 26.3 | 761386 | 761386 | 0 | ingress=2 |
| [metrics_standard_scrape](/performance/scenarios.md#feature-tax-metrics-standard-scrape-forward-fast) | sync | 70029.9 | 28.5 | 703153 | 703153 | 0 | ingress=2 |
| [dnstap_full](/performance/scenarios.md#feature-tax-dnstap-full-forward-fast) | sync | 68939.9 | 28.9 | 692386 | 692386 | 0 | ingress=2 |
| [metrics_standard_dnstap_full](/performance/scenarios.md#feature-tax-metrics-standard-dnstap-full-forward-fast) | sync | 64339.4 | 31.0 | 645499 | 645499 | 0 | ingress=2 |

</div>
