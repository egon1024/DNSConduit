<div class="perf-chart" markdown="1">

### Feature tax — metrics and dnstap combined (forward_fast)

![Feature tax — metrics and dnstap combined (forward_fast)](generated/metrics-dnstap-combined-forward-fast.svg)

[Download CSV](generated/metrics-dnstap-combined-forward-fast.csv)

| Posture | Runtime | Achieved QPS | Avg latency (ms) | Sent | Completed | Lost | Workers |
| --- | --- | --- | --- | --- | --- | --- | --- |
| [metrics_off](/performance/scenarios.md#feature-tax-metrics-off-forward-fast) | sync | 138760.8 | 2.0 | 1391423 | 1387890 | 3449 | ingress=2 |
| [metrics_standard_scrape](/performance/scenarios.md#feature-tax-metrics-standard-scrape-forward-fast) | sync | 121399.7 | 2.3 | 1217696 | 1214291 | 3449 | ingress=2 |
| [dnstap_full](/performance/scenarios.md#feature-tax-dnstap-full-forward-fast) | sync | 113838.8 | 2.6 | 1142096 | 1138671 | 3425 | ingress=2 |
| [metrics_standard_dnstap_full](/performance/scenarios.md#feature-tax-metrics-standard-dnstap-full-forward-fast) | sync | 106763.8 | 1.7 | 1071331 | 1067929 | 3553 | ingress=2 |

</div>
