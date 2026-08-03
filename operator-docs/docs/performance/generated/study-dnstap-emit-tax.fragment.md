## Evidence

<div class="perf-chart" markdown="1">

### Feature tax — dnstap off / sampled / full (forward_fast)

![Feature tax — dnstap off / sampled / full (forward_fast)](generated/dnstap-off-sampled-full-forward-fast.svg)

[Download CSV](generated/dnstap-off-sampled-full-forward-fast.csv)

| Posture | Runtime | Achieved QPS | Avg latency (ms) | Sent | Completed | Lost | Workers |
| --- | --- | --- | --- | --- | --- | --- | --- |
| [dnstap_off](/performance/scenarios.md#feature-tax-dnstap-off-forward-fast) | sync | 75250.1 | 4.0 | 756222 | 752823 | 3399 | ingress=2 |
| [dnstap_sampled](/performance/scenarios.md#feature-tax-dnstap-sampled-forward-fast) | sync | 72484.3 | 4.3 | 728574 | 725189 | 3385 | ingress=2 |
| [dnstap_full](/performance/scenarios.md#feature-tax-dnstap-full-forward-fast) | sync | 67178.8 | 4.4 | 675731 | 672299 | 3432 | ingress=2 |

</div>
