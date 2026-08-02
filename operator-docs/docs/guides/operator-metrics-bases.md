# Operator metrics bases

[`metrics.base`](/observability/metrics-configurability.md) sets how rich Conduit's built-in metrics are when you scrape the Prometheus metrics endpoint. A value of **`minimal`** keeps query labels coarse and skips timing histograms which constitutes metrics with lower metric cardinality. **`standard`** adds fine labels (`qtype`, `qclass`, `ip_family`), phase and forward timing series, and process gauges — more detail, more series.

Operators usually need to answer: “Is **minimal** enough for my dashboards, or do I need **standard**?” This walkthrough runs the **same DNS traffic** under each base, scrapes `/metrics`, and shows the concrete differences so you can choose a base (or confirm a reload changed cardinality as you expected).

**Prerequisites:** Conduit built; upstream DNS resolver listening on **`127.0.0.1:5300`**; Prometheus scrape available via `curl` (no Prometheus server required for this lab). Enable **`pools[].health`** if you want to confirm health series on **minimal**.

## What you will practice

1. Start Conduit with **`base: minimal`**, send a few queries, and read the scrape
2. Switch to **`base: standard`** (restart or overlay) and repeat under the same traffic pattern
3. Compare label sets on [`conduit_queries_total`](/observability/built-in-metrics.md#conduit_queries_total) and whether phase/forward timing series appear

## What you will see

| Base | Hot-path emphasis | After a few `A` queries you should see… |
|------|-------------------|----------------------------------------|
| **`minimal`** | Volume + failures + lookup + health + topology + meta | [`conduit_queries_total`](/observability/built-in-metrics.md#conduit_queries_total) with **`listener`** + **`protocol`** only; failure counters when applicable; **no** `conduit_phase_duration_seconds`; health gauges when probes run |
| **`standard`** | Rich labels + timing + process | `conduit_queries_total` with **`qtype`**, **`qclass`**, **`ip_family`**; phase and forward histograms/counters |

## 1. Minimal base

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
    health:
      enabled: true
      interval_ms: 1000
metrics:
  enabled: true
  base: minimal
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

Expect lines with labels like `listener="127.0.0.1:15353"` and `protocol="udp"` — **without** `qtype="A"` on the same metric (minimal coarse labels).

Confirm phase histograms are absent:

```bash
curl -sS "http://127.0.0.1:9090/metrics" | grep conduit_phase_duration || echo "no phase histograms (expected for minimal)"
```

After ~1 health interval, confirm health series:

```bash
curl -sS "http://127.0.0.1:9090/metrics" | grep -E 'conduit_backend_health_observed|conduit_probe_results_total' | head
```

Stop Conduit before the next step (or use overlay apply if the control plane is enabled — see below).

## 2. Standard base

Copy the config to `conduit-metrics-standard.yaml` and change the base:

```yaml
metrics:
  enabled: true
  base: standard
  prometheus:
    listen_address: "127.0.0.1:9090"
    path: /metrics
```

With a control plane enabled, you can apply an overlay that only changes `metrics.base` without restart. Otherwise restart Conduit with the new file.

```bash
dig @127.0.0.1 -p 15353 +time=3 profiles-standard.example.com A
curl -sS "http://127.0.0.1:9090/metrics" | grep '^conduit_queries_total'
curl -sS "http://127.0.0.1:9090/metrics" | grep conduit_phase_duration | head
```

Expect qtype/qclass/ip_family on queries and phase/forward timing series present.

!!! tip "Legacy profile alias"
    `metrics.profile: full` still loads and maps to **`base: standard`** (with a deprecation warning). Prefer **`base:`** in new configs.

## Related topics

- [Metrics configurability](/observability/metrics-configurability.md)
- [Metrics beyond bases](/guides/metrics-beyond-bases.md) — categories, collect/emit, granularity after you pick a base
- [Built-in metric registry](/observability/built-in-metric-registry.md)
- [Built-in metrics](/observability/built-in-metrics.md)
- [Metrics](/observability/metrics.md)
- [Metrics configurability — Legacy aliases](/observability/metrics-configurability.md#legacy-profile-alias)
- [Performance findings](/performance/index.md#findings) — directional takeaways (scrape, logging, dnstap, OTLP)
- [Metrics scrape tax](/performance/studies/metrics-scrape-ladder.md) — same-host tax of minimal vs standard scrape
- [Metrics collect vs emit](/performance/studies/metrics-collect-vs-emit.md) — recording vs export
- [Metrics scrape (split_io)](/performance/studies/metrics-scrape-split-io.md)
- [Aggressive scrape cadence](/performance/studies/metrics-scrape-hammer.md)
- [OTLP tax under load](/performance/studies/otlp-tax-under-load.md)
