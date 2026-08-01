<div class="perf-chart" markdown="1">

### Feature tax — OTLP push vs baselines (forward_fast)

![Feature tax — OTLP push vs baselines (forward_fast)](generated/otlp-tax-under-load-forward-fast.svg)

[Download CSV](generated/otlp-tax-under-load-forward-fast.csv)

| Posture | Runtime | Achieved QPS | Avg latency (ms) | Sent | Completed | Lost | Workers |
| --- | --- | --- | --- | --- | --- | --- | --- |
| [metrics_off](/performance/scenarios.md#feature-tax-metrics-off-forward-fast) | sync | 138760.8 | 2.0 | 1391423 | 1387890 | 3449 | ingress=2 |
| [metrics_otlp_push](/performance/scenarios.md#feature-tax-metrics-otlp-push-forward-fast) | sync | 116878.8 | 2.6 | 1172470 | 1169073 | 3397 | ingress=2 |
| [metrics_standard_scrape](/performance/scenarios.md#feature-tax-metrics-standard-scrape-forward-fast) | sync | 121399.7 | 2.3 | 1217696 | 1214291 | 3449 | ingress=2 |

</div>
