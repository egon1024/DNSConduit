## Evidence

<div class="perf-chart" markdown="1">

### Feature tax — metrics and dnstap combined (forward_fast)

![Feature tax — metrics and dnstap combined (forward_fast)](generated/metrics-dnstap-combined-forward-fast.svg)

[Download CSV](generated/metrics-dnstap-combined-forward-fast.csv)

| Posture | Runtime | Achieved QPS | Avg latency (ms) | Sent | Completed | Lost | Workers |
| --- | --- | --- | --- | --- | --- | --- | --- |
| [metrics_off](/performance/scenarios.md#feature-tax-metrics-off-forward-fast) | sync | 75250.1 | 26.5 | 754909 | 754909 | 0 | ingress=2 |
| [metrics_standard_scrape](/performance/scenarios.md#feature-tax-metrics-standard-scrape-forward-fast) | sync | 69858.0 | 28.6 | 700756 | 700756 | 0 | ingress=2 |
| [dnstap_full](/performance/scenarios.md#feature-tax-dnstap-full-forward-fast) | sync | 67911.4 | 29.4 | 681855 | 681855 | 0 | ingress=2 |
| [metrics_standard_dnstap_full](/performance/scenarios.md#feature-tax-metrics-standard-dnstap-full-forward-fast) | sync | 63930.1 | 31.2 | 641638 | 641638 | 0 | ingress=2 |

</div>
