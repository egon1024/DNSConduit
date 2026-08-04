<div class="perf-chart" markdown="1">

### Feature tax — OTLP push vs baselines (forward_fast)

![Feature tax — OTLP push vs baselines (forward_fast)](generated/otlp-tax-under-load-forward-fast.svg)

[Download CSV](generated/otlp-tax-under-load-forward-fast.csv)

| Posture | Runtime | Achieved QPS | Avg latency (ms) | Sent | Completed | Lost | Workers |
| --- | --- | --- | --- | --- | --- | --- | --- |
| [metrics_off](/performance/scenarios.md#feature-tax-metrics-off-forward-fast) | sync | 75940.7 | 26.3 | 761386 | 761386 | 0 | ingress=2 |
| [metrics_otlp_push](/performance/scenarios.md#feature-tax-metrics-otlp-push-forward-fast) | sync | 70315.7 | 28.4 | 705512 | 705512 | 0 | ingress=2 |
| [metrics_standard_scrape](/performance/scenarios.md#feature-tax-metrics-standard-scrape-forward-fast) | sync | 70029.9 | 28.5 | 703153 | 703153 | 0 | ingress=2 |

</div>
