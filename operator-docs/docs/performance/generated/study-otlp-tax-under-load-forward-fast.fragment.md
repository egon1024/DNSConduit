<div class="perf-chart" markdown="1">

### Feature tax — OTLP push vs baselines (forward_fast)

![Feature tax — OTLP push vs baselines (forward_fast)](generated/otlp-tax-under-load-forward-fast.svg)

[Download CSV](generated/otlp-tax-under-load-forward-fast.csv)

| Posture | Runtime | Achieved QPS | Avg latency (ms) | Sent | Completed | Lost | Workers |
| --- | --- | --- | --- | --- | --- | --- | --- |
| [metrics_off](/performance/scenarios.md#feature-tax-metrics-off-forward-fast) | sync | 75462.4 | 4.0 | 758529 | 755155 | 3400 | ingress=2 |
| [metrics_otlp_push](/performance/scenarios.md#feature-tax-metrics-otlp-push-forward-fast) | sync | 69165.5 | 4.3 | 695416 | 692002 | 3414 | ingress=2 |
| [metrics_standard_scrape](/performance/scenarios.md#feature-tax-metrics-standard-scrape-forward-fast) | sync | 69628.0 | 4.5 | 700042 | 696579 | 3377 | ingress=2 |

</div>
