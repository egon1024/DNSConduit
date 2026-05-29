# Manual test guide — phase 4b operator metrics

> **Repository:** DNSConduit root. **Ports:** lab range avoids UDP **5353** (mDNS on many Linux desktops).
>
> Phase 4 baseline (tracing, GetTrace, Rhai user metrics): [`phase-4-metrics-tracing.md`](phase-4-metrics-tracing.md).

## Port map

| Role | Address |
|------|---------|
| Conduit DNS (UDP) | `127.0.0.1:15353` |
| Upstream mock (dnsmasq) | `127.0.0.1:15300` → `$UPSTREAM_DNS` |
| Prometheus scrape | `http://127.0.0.1:19090/metrics` |
| Control gRPC | `127.0.0.1:5199` |
| OTLP HTTP (optional) | `http://127.0.0.1:4318/v1/metrics` |

## Configs (this guide)

| Purpose | File |
|---------|------|
| **4b primary** — `profile: full`, two configured backends | [`config/phase-4b-full.yaml`](config/phase-4b-full.yaml) |
| **4b minimal** — compare label gating | [`config/phase-4b-minimal.yaml`](config/phase-4b-minimal.yaml) |
| **4b OTEL** — push parity smoke | [`config/phase-4b-otel.yaml`](config/phase-4b-otel.yaml) |
| **4b outstanding** — optional slow-forward lab | [`config/phase-4b-slow-upstream.yaml`](config/phase-4b-slow-upstream.yaml) |

Fixture equivalents (CI): `tests/fixtures/config/with-metrics-prometheus.yaml` (`profile: full`, one backend).

## Prerequisites

```bash
cd /path/to/DNSConduit
cargo build -p conduit --release
export UPSTREAM_DNS=8.8.8.8   # or your resolver
chmod +x tests/manual/scripts/check-ports.sh
tests/manual/scripts/check-ports.sh
ss -tln | grep -E '19090|5199' || echo "19090 and 5199 appear free"
```

Tools: `dig`, `curl`, `rg` (or `grep`), `dnsmasq`, `nc`, optional `grpcurl`.

## Terminal layout

| Terminal | Role |
|----------|------|
| **A** | dnsmasq on `15300` |
| **B** | Conduit |
| **C** | traffic + scrape |

---

## 0. Start upstream (Terminal A)

```bash
dnsmasq --keep-in-foreground \
  --port=15300 \
  --bind-interfaces \
  --listen-address=127.0.0.1 \
  --server="$UPSTREAM_DNS" \
  --no-hosts --no-resolv --log-queries
```

---

## 1. Extended query labels (`profile: full`)

**Terminal B:**

```bash
cargo run -p conduit -- tests/manual/config/phase-4b-full.yaml
```

Look for listener `127.0.0.1:15353` and Prometheus on `19090`.

**Terminal C:**

```bash
dig @127.0.0.1 -p 15353 +time=3 +tries=1 www.example.com A
dig @127.0.0.1 -p 15353 +time=3 +tries=1 www.example.com AAAA
curl -sS http://127.0.0.1:19090/metrics | rg 'conduit_queries_total|qtype=|qclass=|ip_family='
```

**Expect:**

- `conduit_queries_total` with `listener="127.0.0.1:15353"`, `protocol="udp"`
- `qtype="A"` / `qtype="AAAA"`, `qclass="IN"`, `ip_family="v4"` (IPv4 client)

---

## 2. Per-pool query volume

After §1 traffic:

```bash
curl -sS http://127.0.0.1:19090/metrics | rg 'conduit_queries_by_pool_total'
```

**Expect:** `conduit_queries_by_pool_total{pool="default",...}` ≥ 1.

---

## 3. Responses by `rcode_class`

```bash
dig @127.0.0.1 -p 15353 +time=3 +tries=1 www.example.com A
dig @127.0.0.1 -p 15353 +time=3 +tries=1 this-name-should-not-exist-4b.example. A
curl -sS http://127.0.0.1:19090/metrics | rg 'conduit_responses_total'
```

**Expect:** `rcode_class="NOERROR"` and `rcode_class="NXDOMAIN"` (or `OTHER`).

**Optional SERVFAIL:** stop dnsmasq (Terminal A), then:

```bash
dig @127.0.0.1 -p 15353 +time=3 +tries=1 www.example.com A
curl -sS http://127.0.0.1:19090/metrics | rg 'rcode_class="SERVFAIL"'
```

Restart dnsmasq before later sections.

---

## 4. Parse rejection counters

Still on `phase-4b-full.yaml` (`profile: full`):

**Empty wire (`reason="empty"`):** use Python (or similar). Common `nc` builds **do not send** a zero-length UDP datagram when stdin is empty — they exit without transmitting, so Conduit never increments `empty`.

```bash
python3 -c "import socket; socket.socket(socket.AF_INET, socket.SOCK_DGRAM).sendto(b'', ('127.0.0.1', 15353))"

# wire_error (malformed bytes)
printf '\xff\x00\x01\x02' | nc -u -w1 127.0.0.1 15353

curl -sS http://127.0.0.1:19090/metrics | rg 'conduit_parse_rejected_total'
```

**Expect:** both `reason="empty"` and `reason="wire_error"` ≥ 1 (run the Python line before or after the `printf` line; scrape once at the end).

Other reasons (`not_query`, `no_question`, `multi_question`) are covered by `cargo test -p conduit-core` parse tests.

---

## 5. Scrape-time process and pool gauges

```bash
curl -sS http://127.0.0.1:19090/metrics | rg \
  'conduit_build_info|conduit_start_time_seconds|conduit_config_generation|conduit_pool_backends_configured'
```

**Expect:**

| Metric | Expect |
|--------|--------|
| `conduit_build_info` | `version`, `revision`, `dirty`, `profile` labels; value `1` (see `crates/conduit-metrics/README.md` § Build metadata) |
| `conduit_start_time_seconds` | Unix time ≈ `date +%s` |
| `conduit_config_generation` | `1` on first start |
| `conduit_pool_backends_configured{pool="default"}` | `2` (`phase-4b-full.yaml`) |

**Linux (`profile: full`):**

```bash
curl -sS http://127.0.0.1:19090/metrics | rg 'conduit_process_resident_bytes|conduit_process_open_fds'
```

---

## 6. Outstanding forwards

With Conduit on `phase-4b-full.yaml` (or `phase-4b-slow-upstream.yaml`) and dnsmasq running, the gauge is **always** present for each configured backend (value `0` when nothing is in flight).

**Terminal C — baseline (metric exists at zero):**

```bash
curl -sS http://127.0.0.1:19090/metrics | rg 'conduit_forward_outstanding'
```

**Expect** (one line per backend in config; `phase-4b-full.yaml` lists the same address twice, so you may see two series with the same backend label):

```text
conduit_forward_outstanding{pool="default",backend="127.0.0.1:15300"} 0
```

**Optional — value &gt; 0 while upstream is down:** stop dnsmasq (Terminal A), then:

```bash
dig @127.0.0.1 -p 15353 +time=10 +tries=1 hang-4b.example.com A &
dig @127.0.0.1 -p 15353 +time=10 +tries=1 hang-4b-2.example.com A &
sleep 0.2
curl -sS http://127.0.0.1:19090/metrics | rg 'conduit_forward_outstanding'
wait
```

**Expect:** `conduit_forward_outstanding{pool="default",backend="127.0.0.1:15300"}` ≥ 1 before forwards time out. Restart dnsmasq after.

Parallel `dig` against a **live** upstream usually completes before scrape, so values stay at `0` even though the metric is present — that is normal.

---

## 7. `profile: minimal` vs `full`

Stop Conduit. **Terminal B:**

```bash
cargo run -p conduit -- tests/manual/config/phase-4b-minimal.yaml
```

**Terminal C** — run checks separately (easier to read than one big `rg`):

```bash
dig @127.0.0.1 -p 15353 +time=3 +tries=1 www.example.com A

# 7a — queries: only listener + protocol (no qtype/qclass/ip_family)
curl -sS http://127.0.0.1:19090/metrics | rg '^conduit_queries_total'

# 7b — per-pool counter still on in minimal
curl -sS http://127.0.0.1:19090/metrics | rg '^conduit_queries_by_pool_total'

# 7c — 4b “full only” series are not registered at all in minimal
curl -sS http://127.0.0.1:19090/metrics | rg 'conduit_parse_rejected_total|conduit_responses_total' \
  && echo "UNEXPECTED: full-only metrics present" || echo "OK: no parse/response series"

# 7d — empty packet must not create parse_rejected (metric absent in minimal)
python3 -c "import socket; socket.socket(socket.AF_INET, socket.SOCK_DGRAM).sendto(b'', ('127.0.0.1', 15353))"
curl -sS http://127.0.0.1:19090/metrics | rg 'conduit_parse_rejected_total' \
  && echo "UNEXPECTED" || echo "OK: parse_rejected not exported in minimal"

# 7e — scrape gauges still present
curl -sS http://127.0.0.1:19090/metrics | rg 'conduit_pool_backends_configured|conduit_forward_outstanding'
```

**Expect (minimal) — your §7a output is correct if it looks like:**

```text
conduit_queries_total{listener="127.0.0.1:15353",protocol="udp"} 1
```

No `qtype=`, `qclass=`, or `ip_family=` on that line. **Not** seeing `conduit_parse_rejected_total` or `conduit_responses_total` anywhere on scrape is also correct (those vectors are not registered unless `profile: full`).

**§7b expect:** `conduit_queries_by_pool_total{pool="default",...} 1` (or higher after more queries).

**Compare with full:** restart using `phase-4b-full.yaml`, repeat one `dig`, then:

```bash
curl -sS http://127.0.0.1:19090/metrics | rg '^conduit_queries_total'
```

You should now see `qtype="A"`, `qclass="IN"`, `ip_family="v4"` on the same counter, plus `conduit_responses_total` and (after an empty UDP packet) `conduit_parse_rejected_total`.

---

## 8. Phase 4 regression (forward + phase histograms)

Restart with `phase-4b-full.yaml`, dnsmasq up:

```bash
dig @127.0.0.1 -p 15353 +time=3 +tries=1 test.example.com A
curl -sS http://127.0.0.1:19090/metrics | rg \
  'conduit_forward_attempts_total|conduit_forward_duration_seconds|conduit_phase_duration_seconds'
```

**Expect:** `outcome="success"` and histogram `_bucket` lines.

---

## 9. Metrics disabled (regression)

```bash
cargo run -p conduit -- tests/fixtures/config/metrics-disabled.yaml
```

```bash
dig @127.0.0.1 -p 15353 test.example.com A
curl -sS http://127.0.0.1:19090/metrics
```

**Expect:** `curl` connection refused.

---

## 10. OTEL push (optional)

Run an OpenTelemetry Collector (or compatible receiver) on OTLP HTTP `4318`.

**Terminal B:**

```bash
cargo run -p conduit -- tests/manual/config/phase-4b-otel.yaml
```

Generate traffic (§1). Watch Conduit logs for `otel metrics push ok` or `push failed`. Confirm counters/gauges for 4b families in the collector.

Prometheus scrape remains the easiest way to inspect histogram `_bucket` series; OTLP uses the same underlying Prometheus text.

---

## 11. IPv6 `ip_family` (optional)

Add the `metrics:` block from `phase-4b-full.yaml` to a dual-stack manual config (see [`ipv4-ipv6-forwarding.md`](ipv4-ipv6-forwarding.md)), query via the IPv6 listener, then:

```bash
curl -sS http://127.0.0.1:19090/metrics | rg 'ip_family="v6"'
```

---

## Sign-off scrape (4b)

After §§1–5 on `phase-4b-full.yaml`:

```bash
curl -sS http://127.0.0.1:19090/metrics | rg \
  'conduit_queries_total|conduit_queries_by_pool_total|conduit_parse_rejected_total|conduit_responses_total|conduit_build_info|conduit_start_time_seconds|conduit_config_generation|conduit_pool_backends_configured'
```

---

## Troubleshooting

| Symptom | Check |
|---------|--------|
| No `qtype` labels | Config must use `profile: full` |
| No parse/response counters | Same — only `full` on hot path |
| `listener="unknown"` | Traffic must hit real listener workers |
| No forward metrics | dnsmasq on `15300`, `$UPSTREAM_DNS` set |
| `pool_backends_configured` not 2 | Use `phase-4b-full.yaml` (two backends in pool) |
| OTEL push fails | Collector on `:4318`, endpoint path `/v1/metrics` |
| Only `wire_error`, no `empty` after empty `nc` | `nc` often sends **no** UDP packet on empty stdin; use the Python one-liner in §4 |
| `conduit_forward_outstanding` missing entirely | Rebuild after 4b scrape fix; gauge needs configured backends in snapshot (see §6 baseline) |
| Metric exists but always `0` | Normal with fast upstream; use §6 “upstream down” steps to see &gt; 0 |

## Related

- Metric catalog: `crates/conduit-metrics/README.md`
- Plan: `docs/superpowers/plans/2026-05-27-phase-4b-operator-metrics.md`
