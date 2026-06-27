# Manual test guide — phase 1d dataplane runtime models

> **Repository:** DNSConduit root. **OpenSpec:** `dataplane-runtime-models`.  
> **Execution plan:** `docs/superpowers/plans/2026-06-23-dataplane-runtime-models-execution.md` (DNSConduitCursor).  
> **Gate A:** Complete sections **1–8** (and optional **9**) before operator documentation (Task 12.x).

All commands assume your shell is at the DNSConduit repo root and use the
**release** binary so timings are representative. Each section says which
terminal to run a command in (**A** = upstream, **B** = Conduit, **C** = client).

## Port map

| Role | Address |
|------|---------|
| Conduit DNS (UDP) | `127.0.0.1:15353` |
| Conduit DNS (TCP, per-listener lab) | `127.0.0.1:15354` |
| Upstream mock (dnsmasq) | `127.0.0.1:15300` → `$UPSTREAM_DNS` |
| Prometheus scrape | `http://127.0.0.1:19090/metrics` |
| Control gRPC | `127.0.0.1:5199` |

## Configs

| Purpose | File | Runtime | Ingress threads | timeout_ms / max_txn_ms |
|---------|------|---------|-----------------|--------------------------|
| sync baseline (no `dataplane:`) | [`config/phase-1d-sync-default.yaml`](config/phase-1d-sync-default.yaml) | sync | 1 | 2000 / 5000 |
| split_io production-style | [`config/phase-1d-split-io.yaml`](config/phase-1d-split-io.yaml) | split_io | 2 | 2000 / 5000 |
| slow upstream concurrency lab | [`config/phase-1d-split-io-slow-upstream.yaml`](config/phase-1d-split-io-slow-upstream.yaml) | split_io | 2 | 10000 / 15000 |
| named backends + metrics | [`config/phase-1d-split-io-named-backends.yaml`](config/phase-1d-split-io-named-backends.yaml) | split_io | 2 | 2000 / 5000 |
| per-listener overrides | [`config/phase-1d-per-listener.yaml`](config/phase-1d-per-listener.yaml) | split_io | udp 4 / tcp 1 | 2000 / 5000 |
| invalid runtime (validate only) | [`config/phase-1d-invalid-runtime.yaml`](config/phase-1d-invalid-runtime.yaml) | (rejected) | — | — |

Related: [`phase-4b-slow-upstream.yaml`](config/phase-4b-slow-upstream.yaml), [`phase-4b-operator-metrics.md`](phase-4b-operator-metrics.md).

## Prerequisites

```bash
cd /path/to/DNSConduit
cargo build -p conduit --release
export UPSTREAM_DNS=8.8.8.8
chmod +x tests/manual/scripts/check-ports.sh
tests/manual/scripts/check-ports.sh   # all lab ports must report "free"
```

If `check-ports.sh` reports a port **IN USE**, stop the process holding it
(often a leftover Conduit from a previous section) before continuing:

```bash
pgrep -af 'target/release/conduit' || true
```

Tools required: `dig`, `curl`, `rg`, `dnsmasq`. Optional: `grpcurl`, `conduitctl`.

### Logging

- Default (`RUST_LOG` unset) prints `INFO`, which includes the startup summary
  and per-listener bind lines used as checks below.
- Use `RUST_LOG=debug` only where a section asks for it (drain outcome, per-query
  debug). Prefix the Conduit command, e.g. `RUST_LOG=debug cargo run -p conduit --release -- <config>`.

## Metrics validation

Every config in this guide except `phase-1d-invalid-runtime.yaml` enables the
Prometheus endpoint at `http://127.0.0.1:19090/metrics` with
`metrics.profile: full`. Sections below scrape it with `curl` to corroborate the
`dig` results. Keep a **Terminal C** scrape handy:

```bash
# Full snapshot of Conduit's own metrics (filter out Go/process noise):
curl -sS http://127.0.0.1:19090/metrics | rg '^conduit_'

# A reusable helper for one query then a focused scrape:
scrape() { curl -sS http://127.0.0.1:19090/metrics | rg "$1"; }
```

Counters are cumulative for the process lifetime, so prefer **scrape → send
traffic → scrape again** and compare deltas. Restarting Conduit resets them.

### Metric reference (key series for phase 1d)

| Metric | Type | Labels | Validates |
|--------|------|--------|-----------|
| `conduit_queries_total` | counter | `listener`, `protocol`, `qtype`, `qclass`, `ip_family` | Ingress accepted a query |
| `conduit_responses_total` | counter | `listener`, `protocol`, `rcode`, `ip_family` | Client got a reply + rcode |
| `conduit_queries_by_pool_total` | counter | `pool` | Route stage selected a pool |
| `conduit_forward_attempts_total` | counter | `pool`, `backend`, `outcome` (`success`/`error`) | Upstream attempt + backend label |
| `conduit_forward_errors_total` | counter | `pool`, `reason` (e.g. `timeout`) | Forward failures |
| `conduit_forward_duration_seconds` | histogram | `pool`, `backend` | Upstream RTT |
| `conduit_forward_outstanding` | gauge | `pool`, `backend` | In-flight forwards right now |
| `conduit_phase_duration_seconds` | histogram | `phase` (`forward`, `wait_response`, …) | Time per orchestrator phase |
| `conduit_slots_in_use` | gauge | — | Transaction slots in use (full profile) |
| `conduit_slots_capacity` | gauge | — | Configured slot capacity |
| `conduit_slot_pool_exhausted_total` | counter | — | Slot acquire failures at capacity |
| `conduit_retries_total` | counter | `pool` | Retry transitions |
| `conduit_pool_backends_configured` | gauge | `pool` | Backends configured per pool |
| `conduit_listener_info` | gauge (=1) | `listener`, `address`, `name`, `protocol`, `ip_family`, `reuse_port` | Listener identity; join on `listener`,`protocol` |
| `conduit_listener_ingress_threads` | gauge | `listener`, `protocol` | Resolved ingress threads per listener |
| `conduit_listener_rcvbuf_bytes` | gauge | `listener`, `protocol` | Resolved socket rcvbuf (0 = OS default) |
| `conduit_backend_info` | gauge (=1) | `pool`, `backend`, `address`, `name` | Backend identity; join on `pool`,`backend` |
| `conduit_backend_weight` | gauge | `pool`, `backend` | Effective load-balancing weight |
| `conduit_config_generation` | gauge | — | Active config generation (overlay) |

> **Label note:** `listener` and `backend` both show the configured **name** when
> set, otherwise the bind/`ip:port` address. So an unnamed listener appears as
> `listener="127.0.0.1:15353"` and a named one (`name: lab-udp`) as
> `listener="lab-udp"`; likewise `backend="resolver-east"` vs `backend="127.0.0.1:15300"`.
>
> To recover **both** the name and the address, join against the identity metrics
> `conduit_listener_info{listener,address,name,protocol,ip_family,reuse_port}` and
> `conduit_backend_info{pool,backend,address,name}` (each always `1`), e.g.:
>
> ```promql
> conduit_queries_total * on(listener,protocol) group_left(address,name) conduit_listener_info
> conduit_forward_attempts_total * on(pool,backend) group_left(address,name) conduit_backend_info
> ```
>
> (Join listeners on `listener,protocol` so UDP/TCP entries that share a bind
> address stay distinct.) Numeric per-listener settings are separate gauges
> (`conduit_listener_ingress_threads`, `conduit_listener_rcvbuf_bytes`), and
> backend weight is `conduit_backend_weight` — values belong in gauges, not labels.
>
> These series are config-derived (refreshed each scrape), so they also reflect
> overlay changes. They are emitted by the full `conduit` process, not by the
> dataplane unit-test harness.

## Terminal layout

| Terminal | Role |
|----------|------|
| **A** | dnsmasq on `15300` |
| **B** | Conduit (restarted per section with the section's config) |
| **C** | traffic, scrape, overlay |

To switch configs, stop Conduit in **Terminal B** with `Ctrl+C` and start it
again with the new config path.

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

**Verify (Terminal C):**

```bash
dig @127.0.0.1 -p 15300 +time=3 +tries=1 www.example.com A
```

**Expect:** `status: NOERROR` and an `ANSWER SECTION`. Leave dnsmasq running
except where a section explicitly says to stop it.

---

## 1. Sync default regression

**Config:** `phase-1d-sync-default.yaml` (no `dataplane:` block).

**Terminal B:**

```bash
cargo run -p conduit --release -- tests/manual/config/phase-1d-sync-default.yaml
```

**Expect in Terminal B startup logs:**

- A summary line containing `dataplane_runtime="sync"`, e.g.:

  ```text
  INFO ... dataplane startup summary generation=0 dataplane_runtime="sync" listeners=1 pools=1 ...
  ```

- Exactly **one** bind line (1 ingress thread):

  ```text
  INFO ... Starting listening on 127.0.0.1:15353 udp
  ```

**Terminal C:**

```bash
dig @127.0.0.1 -p 15353 +time=3 +tries=1 www.example.com A
```

**Expect:** `status: NOERROR` with an `ANSWER SECTION`; latency, retries, and
default-level logs indistinguishable from pre-1d behavior. (CI runs `make test`
for the automated sync regression.)

**Metrics check (Terminal C):**

```bash
curl -sS http://127.0.0.1:19090/metrics | rg 'conduit_queries_total|conduit_responses_total|conduit_forward_attempts_total'
```

**Expect:**

- `conduit_queries_total{listener="127.0.0.1:15353",protocol="udp",...} 1` (≥ number of digs)
- `conduit_responses_total{...,rcode="NOERROR",...} 1`
- `conduit_forward_attempts_total{pool="default",backend="127.0.0.1:15300",outcome="success"} 1`

**Pass / fail:** ___________

---

## 2. split_io basic forward

**Config:** `phase-1d-split-io.yaml`.

**Terminal B** (stop the section 1 process first):

```bash
cargo run -p conduit --release -- tests/manual/config/phase-1d-split-io.yaml
```

**Expect in Terminal B startup logs:**

- Summary line containing `dataplane_runtime="split_io"`:

  ```text
  INFO ... dataplane startup summary generation=0 dataplane_runtime="split_io" listeners=1 pools=1 ...
  ```

- Exactly **two** bind lines (ingress `threads: 2`):

  ```text
  INFO ... Starting listening on 127.0.0.1:15353 udp
  INFO ... Starting listening on 127.0.0.1:15353 udp
  ```

  > Note: the summary line does **not** list policy/io worker counts; thread
  > pools are verified by the bind-line count (ingress) and by section 6.

**Terminal C:**

```bash
dig @127.0.0.1 -p 15353 +time=3 +tries=1 www.example.com A
```

**Expect:** `status: NOERROR` with an `ANSWER SECTION`. With `RUST_LOG=debug`,
Terminal B logs a `query complete ... rcode=NOERROR` line for the query.

**Metrics check (Terminal C):**

```bash
curl -sS http://127.0.0.1:19090/metrics \
  | rg 'conduit_queries_by_pool_total|conduit_responses_total|conduit_forward_attempts_total|conduit_forward_duration_seconds_count|conduit_phase_duration_seconds_count|conduit_slots_in_use|conduit_slots_capacity'
```

**Expect (split_io confirms the suspend/resume path is exercised):**

- `conduit_queries_by_pool_total{pool="default"} 1` (≥ digs sent)
- `conduit_responses_total{...,listener="lab-udp",...,rcode="NOERROR",...} 1`
  — this config names the listener (`name: lab-udp`), so the `listener` label is
  the **name**, not `127.0.0.1:15353`.
- `conduit_forward_attempts_total{backend="127.0.0.1:15300",outcome="success",pool="default"} 1`
  — split_io records the forward attempt on the WaitResponse resume, the same as
  the sync runtime. (Backend is unnamed here, so the label is the address.)
- `conduit_forward_duration_seconds_count{backend="127.0.0.1:15300",pool="default"} 1`
  — upstream RTT is observed for the completed forward.
- `conduit_phase_duration_seconds_count{phase="forward"} 1` **and**
  `conduit_phase_duration_seconds_count{phase="wait_response"} 1` (split_io parks
  the wait leg as its own phase)
- `conduit_slots_in_use 0` after the query completes; `conduit_slots_capacity 1024`

**Pass / fail:** ___________

---

## 3. split_io concurrency (core acceptance)

**Goal:** show that a slow upstream parks a query **without** blocking ingress,
so a second query is not serialized behind the first.

**Config:** `phase-1d-split-io-slow-upstream.yaml` (`timeout_ms: 10000`).

1. **Terminal A:** stop dnsmasq with `Ctrl+C` so forwards to `127.0.0.1:15300`
   get **no reply** and park until the 10 s forward timeout. (This makes the
   "slow upstream" deterministic; restart dnsmasq after this section.)
2. **Terminal B:** start the slow-upstream config:

   ```bash
   cargo run -p conduit --release -- tests/manual/config/phase-1d-split-io-slow-upstream.yaml
   ```

3. **Terminal C:** send two queries ~0.5 s apart, each timed:

   ```bash
   ( /usr/bin/time -f 'A %e s' dig @127.0.0.1 -p 15353 +time=12 +tries=1 slow-a.example.com A >/dev/null ) &
   sleep 0.5
   ( /usr/bin/time -f 'B %e s' dig @127.0.0.1 -p 15353 +time=12 +tries=1 slow-b.example.com A >/dev/null ) &
   wait
   ```

**Expect (split_io):** both `A` and `B` finish at roughly the **same** time
(≈10 s each; `B` ≈0.5 s after `A`), because both park concurrently. They return
`SERVFAIL` (upstream down) — timing, not rcode, is the signal here.

**Metrics check — prove concurrency (Terminal C, while A and B are both in flight):**

```bash
# Run this in a separate Terminal C shell during the ~10 s parked window:
curl -sS http://127.0.0.1:19090/metrics | rg 'conduit_forward_outstanding|conduit_slots_in_use'
```

**Expect during the window:**

- `conduit_forward_outstanding{pool="default",backend="127.0.0.1:15300"} 2`
  (both forwards parked at once — the core split_io property)
- `conduit_slots_in_use 2`

After timeout, scrape once more and confirm both return to `0`, and that the
timeouts were recorded:

```bash
curl -sS http://127.0.0.1:19090/metrics | rg 'conduit_forward_errors_total|conduit_forward_attempts_total|conduit_forward_outstanding'
```

**Expect:**

- `conduit_forward_errors_total{pool="default",reason="timeout"} 2`
- `conduit_forward_attempts_total{backend="127.0.0.1:15300",outcome="error",pool="default"} 2`
  — timeouts are attributed to the **real pool** (`default`), not `pool="unknown"`.
- `conduit_forward_outstanding{...} 0`

**Compare (sync):** stop Conduit, edit a copy of `phase-1d-sync-default.yaml` to
set `forward.timeout_ms: 10000`, start it, and repeat step 3. With sync's single
ingress thread, `B` waits behind `A` and finishes ≈20 s after start (≈2×), and a
mid-window scrape shows `conduit_forward_outstanding ... 1` (only one in flight at
a time). Record the observed split_io vs sync difference.

3b. **Terminal A:** restart dnsmasq (section 0 command) before later sections.

**Pass / fail:** ___________

---

## 4. Slot and forward_outstanding metrics

**Config:** `phase-1d-split-io-slow-upstream.yaml` (metrics enabled, Prometheus on
`19090`).

With a query parked on the slow upstream (reuse the section 3 method: stop
dnsmasq, start Conduit, send one `dig ... slow.example.com A &`), scrape during
the wait:

```bash
curl -sS http://127.0.0.1:19090/metrics | rg 'conduit_slots_|conduit_forward_outstanding|conduit_slot_pool_exhausted'
```

**Expect while the forward is pending:**

- `conduit_forward_outstanding{...} >= 1`
- `conduit_slots_in_use >= 1` (parked `IoWait` slot counted)
- `conduit_slots_capacity 1024` (from `orchestrator.txn_table_capacity`)
- `conduit_slot_pool_exhausted_total 0` (no exhaustion under light load)

Scrape again after the query finishes (or upstream recovers):

**Expect:** `conduit_forward_outstanding` and `conduit_slots_in_use` return to `0`.

Restart dnsmasq afterward.

**Pass / fail:** ___________

---

## 5. Named backend metrics label

**Config:** `phase-1d-split-io-named-backends.yaml` (backend `name: resolver-east`,
`address: 127.0.0.1:15300`).

**Terminal B:**

```bash
cargo run -p conduit --release -- tests/manual/config/phase-1d-split-io-named-backends.yaml
```

> **Important:** queries must go **through Conduit** on port **`15353`**, not to
> the upstream on `15300`. A `dig @127.0.0.1 -p 15300 …` only tests dnsmasq and
> records nothing in Conduit. Also, `conduit_forward_attempts_total` is a labeled
> series with **no samples until the first forward**, so an empty `rg` result
> means no query has been forwarded yet (not that the metric is missing).

**Terminal C:** drive a few queries **through Conduit**, then scrape:

```bash
for i in $(seq 1 5); do dig @127.0.0.1 -p 15353 +time=3 +tries=1 www.example.com A >/dev/null; done
curl -sS http://127.0.0.1:19090/metrics | rg 'conduit_forward_attempts_total|conduit_forward_duration'
```

**Expect:** the `backend` label is the configured **name** (`resolver-east`), e.g.

- `conduit_forward_attempts_total{backend="resolver-east",outcome="success",pool="default"} 5`
- `conduit_forward_duration_seconds_count{backend="resolver-east",pool="default"} 5`

The address `127.0.0.1:15300` should **not** appear as a `backend` label. (In
`split_io`, success and RTT are recorded on the WaitResponse resume; the same
name resolution applies to timeouts, so a dead upstream shows
`backend="resolver-east",outcome="error"` rather than the address.)

**Backend identity (recover the address too) and weight:**

```bash
curl -sS http://127.0.0.1:19090/metrics | rg 'conduit_backend_info|conduit_backend_weight'
```

**Expect:**

- `conduit_backend_info{address="127.0.0.1:15300",backend="resolver-east",name="resolver-east",pool="default"} 1`
  — the `backend` join key plus both the `address` and `name`.
- `conduit_backend_weight{backend="resolver-east",pool="default"} 100` (or the
  configured/overlaid weight) — numeric weight lives in its own gauge.

**Pass / fail:** ___________

---

## 6. Per-listener overrides

**Config:** `phase-1d-per-listener.yaml` — UDP `:15353` (`threads: 4`,
`reuse_port: true`) and TCP `:15354` (`threads: 1`), with a global default of 2.

**Terminal B:**

```bash
cargo run -p conduit --release -- tests/manual/config/phase-1d-per-listener.yaml
```

**Verify per-listener thread override via bind lines.** One
`Starting listening on …` line is logged per spawned ingress thread, so the
counts reflect the per-listener overrides (not the global default):

```bash
# In another shell, capture startup logs to a file, or pipe Terminal B output:
#   cargo run ... 2>&1 | tee /tmp/per-listener.log
rg -c 'Starting listening on 127.0.0.1:15353 udp' /tmp/per-listener.log   # expect 4
rg -c 'Starting listening on 127.0.0.1:15354 tcp' /tmp/per-listener.log   # expect 1
```

**Terminal C:** both protocols answer on their ports:

```bash
dig @127.0.0.1 -p 15353       +time=3 +tries=1 www.example.com A   # UDP
dig @127.0.0.1 -p 15354 +tcp  +time=3 +tries=1 www.example.com A   # TCP
```

**Expect:** 4 UDP bind lines, 1 TCP bind line, and `NOERROR` on both queries.

**Metrics check — per-listener labels (Terminal C):**

```bash
curl -sS http://127.0.0.1:19090/metrics | rg 'conduit_queries_total|conduit_responses_total'
```

**Expect** the named listeners appear as distinct `listener` label values (the
configured **names**, since both listeners set `name`):

- `conduit_queries_total{listener="public-udp",protocol="udp",...} 1`
- `conduit_queries_total{listener="lab-tcp",protocol="tcp",...} 1`
- matching `conduit_responses_total{listener="public-udp",...,rcode="NOERROR",...}`
  and `{listener="lab-tcp",...}`

**Listener identity (recover the bind address too) and resolved ingress settings:**

```bash
curl -sS http://127.0.0.1:19090/metrics \
  | rg 'conduit_listener_info|conduit_listener_ingress_threads|conduit_listener_rcvbuf_bytes'
```

**Expect** one `1`-valued `_info` row per listener (join key + address, name,
protocol, ip_family, reuse_port), plus the numeric settings as their own gauges:

- `conduit_listener_info{address="127.0.0.1:15353",ip_family="v4",listener="public-udp",name="public-udp",protocol="udp",reuse_port="true"} 1`
- `conduit_listener_info{address="127.0.0.1:15354",ip_family="v4",listener="lab-tcp",name="lab-tcp",protocol="tcp",reuse_port="false"} 1`
- `conduit_listener_ingress_threads{listener="public-udp",protocol="udp"} 4` and
  `conduit_listener_ingress_threads{listener="lab-tcp",protocol="tcp"} 1` (the
  per-listener overrides verified by the bind-line counts above)

**Pass / fail:** ___________

---

## 7. Overlay patch by backend name

**Config:** `phase-1d-split-io-named-backends.yaml` (control plane enabled on
`127.0.0.1:5199`). Requires `conduitctl`.

Create `overlay-weight.yaml` (note: no `address`, patched by `name`):

```yaml
schema_version: 1
pools:
  - name: default
    backends:
      - name: resolver-east
        weight: 10
```

**Terminal C:**

```bash
conduitctl apply --file overlay-weight.yaml
conduitctl export | rg -A5 'backends:'
```

**Expect:** `resolver-east` effective `weight: 10` without repeating `address`.

**Metrics check — config generation advanced (Terminal C):**

```bash
# Scrape before and after `conduitctl apply` and compare:
curl -sS http://127.0.0.1:19090/metrics | rg 'conduit_config_generation'
```

**Expect:** `conduit_config_generation` increases by 1 after a successful apply
(e.g. `0` → `1`). It does **not** change after the rejected apply below.

**Negative check** — patch an unknown name:

```bash
printf 'schema_version: 1\npools:\n  - name: default\n    backends:\n      - name: does-not-exist\n        weight: 5\n' > overlay-bad.yaml
conduitctl apply --file overlay-bad.yaml
```

**Expect:** apply **rejected** (unknown backend `name`; no append).

**Pass / fail:** ___________

---

## 8. TCP client under split_io

**Config:** `phase-1d-per-listener.yaml` (has the TCP listener on
`127.0.0.1:15354`).

**Terminal C** (with that config running from section 6):

```bash
dig @127.0.0.1 -p 15354 +tcp +time=3 +tries=1 www.example.com A
```

**Expect:** `status: NOERROR` with an `ANSWER SECTION`; no hang on the
connection-owner / reply path. Repeat a few times to confirm no stall on
subsequent TCP queries.

**Metrics check (Terminal C):**

```bash
curl -sS http://127.0.0.1:19090/metrics | rg 'protocol="tcp"'
```

**Expect:** TCP traffic is counted under the TCP listener, with the configured
`name` as the `listener` label, e.g.
`conduit_queries_total{listener="lab-tcp",protocol="tcp",...}` and
`conduit_responses_total{listener="lab-tcp",protocol="tcp",rcode="NOERROR",...}`
increment with each `+tcp` query.

**Pass / fail:** ___________

---

## 9. Drain on shutdown smoke (optional)

Drain runs automatically on process shutdown: the supervisor waits for in-flight
transaction slots to drain (up to `orchestrator.max_txn_duration_ms`) before
tearing down listeners.

1. **Terminal A:** stop dnsmasq so a forward will park (as in section 3).
2. **Terminal B:** start split_io with the slow-upstream config and debug logs:

   ```bash
   RUST_LOG=debug cargo run -p conduit --release -- tests/manual/config/phase-1d-split-io-slow-upstream.yaml
   ```

3. **Terminal C:** send a query that parks on the dead upstream:

   ```bash
   dig @127.0.0.1 -p 15353 +time=20 +tries=1 drain-test.example.com A &
   ```

4. **Terminal B:** while the query is in flight, press `Ctrl+C` once to trigger
   shutdown.

**Expect (Terminal B logs one of):**

- `dataplane slots drained` (debug) — the parked slot completed/timed out within
  the drain window, then shutdown proceeded; or
- `dataplane drain timed out; forcing shutdown remaining=<n> timeout_ms=<ms>`
  (warn) — drain window elapsed with slots still in flight.

Either outcome is a pass: shutdown is orderly and the in-flight slot is accounted
for (no panic, no indefinite hang). Restart dnsmasq afterward.

> Protocol-scoped `DrainFilter` (UDP-only leaves TCP slots) is covered by the
> automated unit test `udp_only_drain_ignores_tcp_slots`; there is no CLI flag
> for selective drain yet. Listener-scoped drain is a deferred future feature.

**Pass / fail:** ___________ / N/A

---

## Gate A sign-off

| Reviewer | Date | Notes |
|----------|------|-------|
| egon | 2026-06-26 | Signed off — required sections 1–8 validated (§9 drain smoke optional); cleared to start operator documentation (Task 12.x) |

**Approved to start operator documentation (Task 12.x):** **yes**

---

## Changelog

| Date | Change |
|------|--------|
| 2026-06-23 | Initial scaffold for phase 1d Gate A |
| 2026-06-25 | Made sections explicit: exact startup/bind log checks, per-listener thread verification via bind-line counts, deterministic slow-upstream concurrency procedure with timing, concrete metric expectations, drain-on-shutdown smoke for the implemented drain hook |
| 2026-06-25 | Added Metrics validation section (metric reference table + scrape helper) and per-section `curl` metric checks (queries/responses, pool routing, phase durations, forward_outstanding concurrency proof, named listener/backend labels, config_generation on overlay); enabled Prometheus in `phase-1d-per-listener.yaml` |
| 2026-06-25 | Added split_io forward success/RTT checks (§2, §5) and timeout `pool="default"` checks (§3) after fixing split_io to record `conduit_forward_attempts_total`/`conduit_forward_duration_seconds` on the WaitResponse resume. Clarified §5: query via Conduit (`15353`), and labeled vectors emit no samples until first forward |
| 2026-06-25 | `listener` metric label now uses the configured listener `name` when set (else bind address), matching backend label behavior. Updated label note and §2/§6/§8 expectations to use names (`lab-udp`, `public-udp`, `lab-tcp`) |
| 2026-06-25 | Added `conduit_listener_info` and `conduit_backend_info` identity metrics (always `1`) carrying both the join-key label and the `address`+`name`, so dashboards can recover both via a PromQL join. Added reference rows, a join example in the label note, and `_info` checks to §5 (backend) and §6 (listener) |
| 2026-06-25 | Enriched `conduit_listener_info` with `protocol`/`ip_family`/`reuse_port` (protocol disambiguates UDP/TCP on a shared address; join now `on(listener,protocol)`); added numeric gauges `conduit_listener_ingress_threads`, `conduit_listener_rcvbuf_bytes`, and `conduit_backend_weight` (numbers as gauge values, not labels). Updated reference table and §5/§6 checks |
| 2026-06-26 | Gate A signed off (egon) — required sections 1–8 validated (§9 drain smoke optional); approved to start operator documentation (Task 12.x) |
```

