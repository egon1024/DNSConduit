# Metrics and tracing

End-to-end lab setup: enable built-in [metrics](/observability/metrics.md) (Prometheus scrape), optional [pipeline tracing](/observability/tracing.md), and the [control plane](/glossary/index.md#control-plane) so you can fetch traces with `conduitctl trace`. This guide does not cover [event export](/observability/event-export.md) or OTEL push — see [Metrics](/observability/metrics.md) for OTLP configuration.

**Prerequisites:** Conduit built and on your `PATH`; an upstream DNS listener on **`127.0.0.1:5300`** (or adjust the pool backend below). Follow [Install and run](/getting-started/install-and-run.md) if you have not started Conduit yet.

## What you will verify

1. Prometheus-format metrics at an HTTP scrape endpoint
2. Per-query counters increment after DNS traffic
3. Pipeline trace events for a matching query via `conduitctl trace`

## 1. Write the config

Save as `conduit-obs-lab.yaml` (ports match common manual-test layouts: DNS **`15353`**, metrics **`19090`**, control **`5199`**):

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
control:
  listen_address: "127.0.0.1:5199"
metrics:
  enabled: true
  profile: full
  prometheus:
    listen_address: "127.0.0.1:19090"
    path: /metrics
tracing:
  enabled: true
  activation:
    selectors:
      - type: qtype
        value: A
    sample_rate: 1.0
  output:
    log_json: false
logging:
  level: info
  output: stderr
```

| Block | Role in this lab |
|-------|------------------|
| `control:` | Required for `conduitctl trace` |
| `metrics:` | Hot-path recording + Prometheus scrape on **`19090`** |
| `tracing:` | Trace every **`A`** query (`sample_rate: 1.0`) |
| `logging:` | Default **`info`** — quiet under load; bump to **`debug`** if you need `txn_id` on each query |

Validate before start:

```bash
conduitctl validate --file conduit-obs-lab.yaml
```

## 2. Start Conduit

```bash
conduit /path/to/conduit-obs-lab.yaml
```

Confirm startup in the log: **`dataplane startup summary`**, listener on **`15353`**, and no bind errors for **`19090`** or **`5199`**.

!!! note "Restart after observability changes"
    Metrics export listeners and tracing activation are established at **process start**. If you change `metrics:`, `tracing:`, or add `control:` later, **restart** Conduit — reload alone does not rebind scrape or recompile tracing. See [Observability — Changing observability config](/observability/index.md#changing-observability-config).

## 3. Smoke-test metrics

Before traffic, scrape should respond (gauges may be zero):

```bash
curl -sS "http://127.0.0.1:19090/metrics" | head
```

Send a query:

```bash
dig @127.0.0.1 -p 15353 +time=3 lab.example.com A
```

Scrape again and look for query counters (names depend on profile — **`full`** includes `qtype` labels):

```bash
curl -sS "http://127.0.0.1:19090/metrics" | grep conduit_queries
```

Expect [`conduit_queries_total`](/observability/built-in-metrics.md#conduit_queries_total) and related series to increment. Series reference: [Built-in metrics](/observability/built-in-metrics.md).

## 4. Fetch a pipeline trace

The first completed query on a worker often has **`txn_id=1`**. If `conduitctl trace 1` returns not found, set **`logging.level: debug`**, restart, send another query, and read **`txn_id`** from a **`query complete`** line — see [Logging — Per-query summaries](/observability/logging.md#per-query-summaries).

```bash
conduitctl trace 1
```

Expect phase lines such as **`route`**, **`forward`**, and **`send`** with elapsed microseconds and pool/backend fields. Alternative: gRPC **`GetTrace`** — [gRPC and conduitctl — trace](/control-plane/grpc-and-conduitctl.md#trace).

Traces are stored in memory (**5 minute** TTL, **1000** entry cap). Activation must match the query — here, **`A`** queries only.

## 5. Optional checks

| Check | Command / action |
|-------|------------------|
| Config generation gauge | `curl -sS http://127.0.0.1:19090/metrics \| grep conduit_config_generation` |
| Phase histograms (**`full`** profile) | `grep conduit_phase_duration` on scrape output after several queries |
| JSON trace on stderr | Set `tracing.output.log_json: true`, restart, send a matching query; look for `conduit::trace` at **`info`** |

## What to do next

- Compare **`minimal`** vs **`full`** profiles — [Operator metrics profiles](/guides/operator-metrics-profiles.md)
- Add [dnstap event export](/guides/event-export-dnstap.md) for wire-level taps
- Add OTEL push under `metrics.otel` — [Metrics — OTEL](/observability/metrics.md#enabling-export)
- Symptom help — [Troubleshooting — Observability](/troubleshooting/index.md#observability)

## Related topics

- [Observability](/observability/index.md) — signal choice, OTEL naming, reload matrix
- [Architecture and packet path](/concepts/architecture-and-packet-path.md) — pipeline phases traced and metered
- [Configuration model](/control-plane/configuration-model.md) — file layer vs overlay
