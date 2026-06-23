# Operator metrics profiles

Hands-on comparison of **`metrics.profile: minimal`** vs **`full`** on the same DNS traffic. For the series catalog and profile table, see [Built-in metrics — Profiles](/observability/built-in-metrics.md#profiles). For enabling scrape and OTEL, see [Metrics](/observability/metrics.md).

**Prerequisites:** Conduit built; upstream on **`127.0.0.1:5300`**; Prometheus scrape available via `curl` (no Prometheus server required for this lab).

## What you will see

| Profile | Hot-path emphasis | After a few `A` queries you should see… |
|---------|-------------------|----------------------------------------|
| **`minimal`** | Volume + failure counters | [`conduit_queries_total`](/observability/built-in-metrics.md#conduit_queries_total) with **`listener`** + **`protocol`** only; [`conduit_parse_rejected_total`](/observability/built-in-metrics.md#conduit_parse_rejected_total) and [`conduit_forward_errors_total`](/observability/built-in-metrics.md#conduit_forward_errors_total) when failures occur; **no** `conduit_phase_duration_seconds` |
| **`full`** | Rich labels + timing | `conduit_queries_total` with **`qtype`**, **`qclass`**, **`ip_family`**; phase and forward histograms/counters |

Both profiles expose the same scrape-time gauges except Linux process memory/FD gauges (**`full`** only).

## 1. Minimal profile

Save as `conduit-metrics-minimal.yaml`:

```yaml
schema_version: 1
listeners:
  listeners:
    - address: "127.0.0.1:15353"
      protocol: udp
pools:
  - name: default
    backends:
      - address: "127.0.0.1:5300"
metrics:
  enabled: true
  profile: minimal
  prometheus:
    listen_address: "127.0.0.1:9090"
    path: /metrics
```

```bash
conduitctl validate --file conduit-metrics-minimal.yaml
conduit /path/to/conduit-metrics-minimal.yaml
```

Send a few queries:

```bash
dig @127.0.0.1 -p 15353 +time=3 profiles-minimal.example.com A
dig @127.0.0.1 -p 15353 +time=3 profiles-minimal.example.com AAAA
```

Scrape and inspect query counters:

```bash
curl -sS "http://127.0.0.1:9090/metrics" | grep '^conduit_queries_total'
```

Expect lines with labels like `listener="127.0.0.1:15353"` and `protocol="udp"` — **without** `qtype="A"` on the same metric (minimal does not add qtype/qclass/ip_family on the hot path).

Confirm phase histograms are absent:

```bash
curl -sS "http://127.0.0.1:9090/metrics" | grep conduit_phase_duration || echo "no phase histograms (expected for minimal)"
```

Stop Conduit before the next step.

## 2. Full profile

Copy the config to `conduit-metrics-full.yaml` and change only the profile:

```yaml
metrics:
  enabled: true
  profile: full
  prometheus:
    listen_address: "127.0.0.1:9090"
    path: /metrics
```

!!! note "Restart required"
    Profile changes take effect on the hot path only after a **process restart** — reload updates stored config but does not switch recording mode today. See [Metrics — Changing metrics config](/observability/metrics.md#changing-metrics-config).

```bash
conduit /path/to/conduit-metrics-full.yaml
```

Repeat the same `dig` commands (or use new QNAMEs). Scrape again:

```bash
curl -sS "http://127.0.0.1:9090/metrics" | grep '^conduit_queries_total'
```

Expect **`qtype`**, **`qclass`**, and **`ip_family`** labels on query counters. After several queries:

```bash
curl -sS "http://127.0.0.1:9090/metrics" | grep conduit_phase_duration | head
curl -sS "http://127.0.0.1:9090/metrics" | grep conduit_forward_
```

On Linux, **`full`** also exposes process gauges at scrape time:

```bash
curl -sS "http://127.0.0.1:9090/metrics" | grep conduit_process_
```

## 3. Choosing a profile

| Choose **`minimal`** when… | Choose **`full`** when… |
|----------------------------|-------------------------|
| You need query volume, pool mix, response mix, and alertable failure counters | You need per-qtype volume, forward RTT, per-backend attempt counts, or phase timing |
| Cardinality and hot-path cost must stay low (no histograms) | You are operating or debugging upstream and pipeline latency in detail |
| Coarse response buckets are enough | You need fine `rcode` labels and `ip_family` on responses |

Default when `metrics:` is present and `profile` is omitted: **`full`**. Full series list: [Built-in metrics](/observability/built-in-metrics.md).

## Related topics

- [Metrics](/observability/metrics.md) — export paths and OTEL
- [Metrics and tracing](/guides/metrics-and-tracing.md) — combined metrics + tracing lab
- [Troubleshooting — Metrics scrape](/troubleshooting/index.md#observability)
