## Evidence

<div class="perf-chart" markdown="1">

### Feature tax — OTLP push vs baselines (forward_fast)

![Feature tax — OTLP push vs baselines (forward_fast)](generated/otlp-tax-under-load-forward-fast.svg)

[Download CSV](generated/otlp-tax-under-load-forward-fast.csv)

| Posture | Runtime | Achieved QPS | Avg latency (ms) | Sent | Completed | Lost | Workers |
| --- | --- | --- | --- | --- | --- | --- | --- |
| [metrics_off](/performance/scenarios.md#feature-tax-metrics-off-forward-fast) | sync | 75250.1 | 26.5 | 754909 | 754909 | 0 | ingress=2 |
| [metrics_otlp_push](/performance/scenarios.md#feature-tax-metrics-otlp-push-forward-fast) | sync | 69276.6 | 28.8 | 695057 | 695057 | 0 | ingress=2 |
| [metrics_standard_scrape](/performance/scenarios.md#feature-tax-metrics-standard-scrape-forward-fast) | sync | 69858.0 | 28.6 | 700756 | 700756 | 0 | ingress=2 |

</div>
