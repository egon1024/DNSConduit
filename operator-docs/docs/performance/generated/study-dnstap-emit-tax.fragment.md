## Evidence

<div class="perf-chart" markdown="1">

### Feature tax — dnstap off / sampled / full (forward_fast)

![Feature tax — dnstap off / sampled / full (forward_fast)](generated/dnstap-off-sampled-full-forward-fast.svg)

[Download CSV](generated/dnstap-off-sampled-full-forward-fast.csv)

| Posture | Runtime | Achieved QPS | Avg latency (ms) | Sent | Completed | Lost | Workers |
| --- | --- | --- | --- | --- | --- | --- | --- |
| [dnstap_off](/performance/scenarios.md#feature-tax-dnstap-off-forward-fast) | sync | 75505.8 | 26.4 | 757062 | 757062 | 0 | ingress=2 |
| [dnstap_sampled](/performance/scenarios.md#feature-tax-dnstap-sampled-forward-fast) | sync | 71108.5 | 28.0 | 714045 | 714045 | 0 | ingress=2 |
| [dnstap_full](/performance/scenarios.md#feature-tax-dnstap-full-forward-fast) | sync | 67911.4 | 29.4 | 681855 | 681855 | 0 | ingress=2 |

</div>
