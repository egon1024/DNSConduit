# Manual test guide — phase 4 metrics and tracing

> **Repository:** DNSConduit root. **Ports:** lab range avoids UDP **5353** (mDNS on many Linux desktops).

## Port map

| Role | Address |
|------|---------|
| Conduit DNS (UDP) | `127.0.0.1:15353` |
| Upstream mock (dnsmasq) | `127.0.0.1:15300` → your resolver |
| Prometheus scrape | `http://127.0.0.1:19090/metrics` |
| Control gRPC (`GetTrace`, `Health`) | `127.0.0.1:5199` |

Configs:

| Purpose | File |
|---------|------|
| Metrics + Prometheus | `tests/fixtures/config/with-metrics-prometheus.yaml` |
| Tracing only (no `/metrics` listener) | `tests/fixtures/config/with-tracing-selectors.yaml` |
| Metrics + tracing + Prometheus | `tests/fixtures/config/with-metrics-tracing-prometheus.yaml` |
| Metrics disabled | `tests/fixtures/config/metrics-disabled.yaml` |
| Rhai user metrics | `tests/fixtures/config/with-rhai-block-hits.yaml` |

Phase **4b** operator metrics (extended built-ins, parse rejects, scrape gauges): [`phase-4b-operator-metrics.md`](phase-4b-operator-metrics.md) and `tests/manual/config/phase-4b-*.yaml`.

## Prerequisites

```bash
cd /path/to/DNSConduit
cargo build -p conduit --release
# optional: cargo build -p conduit-dnstap-tracer --release
```

Tools: `dig`, `curl`, `grpcurl` (for `GetTrace`), `dnsmasq`, `ss`.

Set a real resolver for the upstream mock:

```bash
export UPSTREAM_DNS=8.8.8.8   # or your resolver
```

## Pre-flight

```bash
chmod +x tests/manual/scripts/check-ports.sh
tests/manual/scripts/check-ports.sh
```

Expect **free** for UDP `15353`, `15300`, and TCP `19090` / `5199` (metrics/control may not be checked by the script; verify with `ss` if needed).

---

## 1. Automated regression (optional)

```bash
make test
```

Phase-4-focused:

```bash
cargo test -p conduit-metrics --test prometheus_scrape
cargo test -p conduit-api --test grpc_get_trace
```

---

## 2. Start upstream mock (Terminal A)

IPv4 dnsmasq forwarding to your resolver:

```bash
dnsmasq --keep-in-foreground \
  --port=15300 \
  --bind-interfaces \
  --listen-address=127.0.0.1 \
  --server="$UPSTREAM_DNS" \
  --no-hosts --no-resolv --log-queries
```

Leave running.

---

## 3. Prometheus scrape and forward metrics (Terminal B + C)

**Terminal B — Conduit:**

```bash
cargo run -p conduit -- tests/fixtures/config/with-metrics-prometheus.yaml
```

Look for listener bind on `127.0.0.1:15353` and metrics HTTP on `19090`.

**Terminal C — traffic and scrape:**

```bash
dig @127.0.0.1 -p 15353 +time=3 +tries=1 test.example.com A
dig @127.0.0.1 -p 15353 +time=3 +tries=1 test.example.com A
curl -sS http://127.0.0.1:19090/metrics | less
```

**Expect in scrape output:**

- `conduit_queries_total` with `listener="127.0.0.1:15353"` and `protocol="udp"`
- `conduit_forward_attempts_total` with `outcome="success"` (when dnsmasq is up)
- `conduit_forward_duration_seconds` histogram buckets for the pool/backend
- `conduit_phase_duration_seconds` (`profile: full`)

Quick check:

```bash
curl -sS http://127.0.0.1:19090/metrics | rg 'conduit_queries_total|conduit_forward_|listener="127.0.0.1:15353"'
```

Stop Conduit with Ctrl+C when done.

---

## 4. Metrics + tracing + GetTrace (combined config)

Stop previous Conduit. **Terminal B:**

```bash
cargo run -p conduit -- tests/fixtures/config/with-metrics-tracing-prometheus.yaml
```

**Terminal C:**

```bash
dig @127.0.0.1 -p 15353 +time=3 +tries=1 test.example.com A
```

First transaction id is usually `1` (incrementing per worker).

**Health (reflection enabled in this fixture):**

```bash
grpcurl -plaintext 127.0.0.1:5199 conduit.v1.ConduitControl/Health
```

**GetTrace** (reflection enabled in this fixture):

```bash
grpcurl -plaintext \
  -d '{"txn_id":"1"}' \
  127.0.0.1:5199 \
  conduit.v1.ConduitControl/GetTrace
```

**Expect:** `"found": true` and `events` including phases such as `route`, `forward`, `send`.

**Unknown transaction id:**

```bash
grpcurl -plaintext \
  -d '{"txn_id":"999999"}' \
  127.0.0.1:5199 \
  conduit.v1.ConduitControl/GetTrace
```

**Expect:** `"found": false`, empty `events`.

Still scrape metrics on the same run:

```bash
curl -sS http://127.0.0.1:19090/metrics | rg conduit_queries_total
```

---

## 5. Rhai user metrics

Use the block-hits fixture (listener already on **15353**, Prometheus on **19090**):

**Terminal B:**

```bash
cargo run -p conduit -- tests/fixtures/config/with-rhai-block-hits.yaml
```

**Terminal C:**

```bash
dig @127.0.0.1 -p 15353 eu.example. A
dig @127.0.0.1 -p 15353 eu.example. A
curl -sS http://127.0.0.1:19090/metrics | rg conduit_user_block_hits
```

**Expect:** `conduit_user_block_hits` with value `2` (cumulative). Requires `eu.example.` in `tests/fixtures/data/geo.csv`.

---

## 6. Metrics disabled

`metrics-disabled.yaml` has `metrics.enabled: false` and **no** Prometheus listener — use this to confirm Conduit still runs DNS without opening `:19090`.

**Terminal B:**

```bash
cargo run -p conduit -- tests/fixtures/config/metrics-disabled.yaml
```

**Terminal C:**

```bash
dig @127.0.0.1 -p 15353 test.example.com A
curl -sS http://127.0.0.1:19090/metrics
```

**Expect:** `curl` fails to connect (connection refused). Automated test `metrics_disabled_leaves_builtin_counters_at_zero` covers in-process counters.

---

## 7. profile: minimal

Use [`config/phase-4b-minimal.yaml`](config/phase-4b-minimal.yaml) or copy `with-metrics-prometheus.yaml` with `metrics.profile: minimal`. For full 4b coverage see [`phase-4b-operator-metrics.md`](phase-4b-operator-metrics.md) §7.

**Expect:** `conduit_queries_total` updates; forward/phase histogram series do not increment on the hot path (see `crates/conduit-metrics/README.md`).

---

## 8. OTEL push (optional)

Add to a metrics config:

```yaml
metrics:
  otel:
    endpoint: "http://127.0.0.1:4318/v1/metrics"
    push_interval_ms: 5000
    resource_attributes:
      service.name: conduit
```

Run an OpenTelemetry Collector listening on `:4318`, start Conduit (e.g. [`config/phase-4b-otel.yaml`](config/phase-4b-otel.yaml)), and watch logs for `otel metrics push ok` or `push failed`. Phase **4b** exports counters, gauges, and histogram summaries from the same Prometheus text as scrape (see `crates/conduit-metrics/README.md`).

---

## Troubleshooting

| Symptom | Check |
|---------|--------|
| `Address already in use` on start | `tests/manual/scripts/check-ports.sh`; avoid port **5353** — use **15353** |
| `curl :19090` fails | Config must include `metrics.prometheus.listen_address` |
| No forward success metrics | dnsmasq on **15300** running; `UPSTREAM_DNS` set |
| `listener="unknown"` in metrics | Traffic must hit real listener workers (not unit-test orchestrator only) |
| GetTrace `found: false` | Use numeric `txn_id` matching first query (`1`, `2`, …); tracing enabled and qtype A |
| SERVFAIL from dig | Backend down or timeout; forward **error** metrics should still increment |

If reflection is disabled (`control.reflection_enabled: false`), call grpcurl with proto files:

```bash
grpcurl -plaintext \
  -import-path proto \
  -proto conduit/v1/control.proto \
  127.0.0.1:5199 \
  conduit.v1.ConduitControl/Health
```

## Related

- IPv6 / dual-stack lab: [`ipv4-ipv6-forwarding.md`](ipv4-ipv6-forwarding.md)
- Rhai cookbook ports: [`tests/fixtures/rhai/README.md`](../fixtures/rhai/README.md)

## Future documentation note

- Add an explicit operator-facing explanation of how `conduit_*_duration_seconds` Prometheus histograms map to latency distributions (bucket/cumulative semantics, plus `_sum` and `_count` interpretation), including why Conduit uses this histogram model.
