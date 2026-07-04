# Manual test guide — phase 1c backend health (Gates A–G + phase exit)

> **Repository:** DNSConduit root. **OpenSpec:** `backend-health-probes` (+ `pool-routing`, `dns-forward` deltas).  
> **Gate A scope:** active probing and health **state only** — Phase A wires probe
> outcomes into per-backend health but does **not** change routing. Confirm
> probes fire, state transitions correctly, the loop is multiplexed, and routing
> is unaffected. Routing-follows-health is Gate B.

All commands assume your shell is at the DNSConduit repo root. Each section says
which terminal to run a command in (**A** = upstream, **B** = Conduit,
**C** = client / observer).

## Port map

| Role | Address |
|------|---------|
| Conduit DNS (UDP) | `127.0.0.1:15353` |
| Upstream mock (dnsmasq) — the **live** backend | `127.0.0.1:15300` → `$UPSTREAM_DNS` |
| **Dead** backend (nothing listening) | `127.0.0.1:15399` |
| Prometheus scrape | `http://127.0.0.1:19090/metrics` |
| Control gRPC | `127.0.0.1:5199` |

## Config

| Purpose | File | Health | Backends |
|---------|------|--------|----------|
| Gate A health lab | [`config/phase-1c-health.yaml`](config/phase-1c-health.yaml) | `enabled`, `interval_ms: 1000`, `rise: 3`, `fall: 2` | `live` (15300), `dead` (15399) |

The pool `default` carries the health block; both backends share equal weight so
§6 can prove routing still hits the dead one.

## Prerequisites

```bash
cd /path/to/DNSConduit
cargo build -p conduit --release
export UPSTREAM_DNS=8.8.8.8
```

Tools required: `dig`, `curl`, `rg`, `dnsmasq`, `tcpdump` (root for capture).

### Logging

Phase A exposes health **only** through INFO log lines (metrics arrive in a later
phase). Each `observed`/`applied` change emits:

```
INFO conduit_dataplane::probe::scheduler: backend health transition pool=… backend=… observed=… applied=…
```

Run Conduit with `RUST_LOG=info` (default) for these. Use
`RUST_LOG=info,conduit_dataplane=debug` for extra probe-loop detail.

## Terminal layout

| Terminal | Role |
|----------|------|
| **A** | dnsmasq on `15300` (the live backend) |
| **B** | Conduit (`config/phase-1c-health.yaml`) |
| **C** | tcpdump, `dig`, log/metric inspection |

---

## 0. Start the live backend (Terminal A)

```bash
dnsmasq --keep-in-foreground \
  --port=15300 \
  --bind-interfaces \
  --listen-address=127.0.0.1 \
  --server="$UPSTREAM_DNS" \
  --no-hosts --no-resolv --log-queries
```

Leave `127.0.0.1:15399` with **nothing** listening — that is the dead backend.

**Verify (Terminal C):**

```bash
dig @127.0.0.1 -p 15300 +time=3 +tries=1 www.example.com A   # expect NOERROR + ANSWER
```

Then start Conduit (Terminal B):

```bash
RUST_LOG=info cargo run -p conduit --release -- tests/manual/config/phase-1c-health.yaml
```

**Expect:** a startup line `backend health probe loop starting … backends=2`.

**Pass / fail:** Pass

---

## 1. Two-backend pool, health enabled (task 3.1)

Confirm the lab is wired: pool `default` has `live` (15300) and `dead` (15399),
health enabled at `interval_ms: 1000`.

**Verify (Terminal C):**

```bash
rg -n 'backends=2' <conduit stdout>           # probe loop covers both backends
```

**Expect:** probe loop reports `backends=2`; Conduit is serving on `127.0.0.1:15353`.

**Pass / fail:** Pass

---

## 2. Probes on the wire (task 3.2)

**Capture (Terminal C, root):**

```bash
sudo tcpdump -ni any -vvv 'udp port 15300 or udp port 15399' &
sleep 6 ; kill %1
```

**Expect:**
- One probe to **each** backend roughly every 1s (jittered up to +20%, so ~1.0–1.2s gaps).
- Each probe carries the configured question `health-probe.example. A`.
- Distinct, varying DNS transaction IDs (fresh qid per probe).

**Pass / fail:** Pass (cadence ≈1s ☑  qname/qtype correct ☑  qids vary ☑)

---

## 3. State transitions: down then up (task 3.3)

Watch the transition log while the dead backend stays dead and the live backend
is killed and restored.

**Observe (Terminal C):** filter Conduit output:

```bash
rg 'backend health transition' <conduit stdout>
```

**Steps:**
1. At startup, `dead` (15399) should reach `observed=Down applied=Down` after `fall` (2) probes (~2–3s).
2. The `live` backend (15300) should reach `observed=Up applied=Up` after `rise` (3) successes.
3. **Kill** dnsmasq (Terminal A, `Ctrl+C`); within `fall × interval` (~2s) `live` logs `… observed=Down applied=Down`.
4. **Restart** dnsmasq; within `rise × interval` (~3s) `live` logs `… observed=Up applied=Up`.

**Expect:** transitions match the rise/fall counts above (down after 2 fails, up after 3 successes).

**Pass / fail:** Pass (dead→down ☑  live kill→down ☑  live restore→up ☑)

---

## 4. Multiplex isolation — one dead backend never delays others (task 3.4)

With `dead` (15399) never answering, the live backend must keep its ~1s cadence
(the dead backend's outstanding probe/timeout must not stall the loop).

**Capture (Terminal C, root):**

```bash
sudo tcpdump -ni any -ttt 'udp port 15300' &   # -ttt prints inter-packet deltas
sleep 10 ; kill %1
```

**Expect:** consecutive probes to `15300` stay ~1.0–1.2s apart throughout, even
though `15399` is dead the whole time (no multi-second gap aligned to the dead
backend's timeout).

**Pass / fail:** Pass

---

## 5. Skip-if-outstanding — at most one in-flight probe per backend (task 3.5)

Point a backend at a **slow** responder and set `timeout_ms` > `interval_ms`, then
confirm no second probe is sent while one is still outstanding.

**Setup:** copy the lab config and raise the timeout above the interval, e.g.:

```bash
sed 's/timeout_ms: 1000/timeout_ms: 4000/' tests/manual/config/phase-1c-health.yaml > /tmp/phase-1c-skip.yaml
```

Make one backend slow (a responder that delays > 1s but < 4s), restart Conduit on
`/tmp/phase-1c-skip.yaml`, and capture that backend's traffic:

```bash
sudo tcpdump -ni any -ttt 'udp port <slow-backend-port>' &
sleep 8 ; kill %1
```

**Expect:** never more than one outstanding probe per backend — a new probe is
sent only after the previous reply or its timeout, so you do **not** see a fresh
probe every 1s while one is still in flight.

**Pass / fail:** Pass

---

## 6. No routing impact — Phase A is state-only (task 3.6)

Send client traffic and confirm the **dead** backend still receives forwarded
queries (routing does not yet read health).

**Traffic (Terminal C):**

```bash
for i in $(seq 1 20); do dig @127.0.0.1 -p 15353 +time=2 +tries=1 example.com A >/dev/null; done
```

**Confirm:** capture upstream and check the dead backend still gets a share:

```bash
sudo tcpdump -ni any 'udp port 15399' &   # dead backend still targeted by forward
sleep 5 ; kill %1
```

**Expect:** forwarded queries still reach `15399` despite it being `applied=Down`
in the health log — proving Phase A does **not** change backend selection. (Some
client queries via the dead backend will fail/retry; that is expected for now.)

**Pass / fail:** Pass

---

## 7. Record results (task 3.7)

Fill in the Pass / fail lines above and the sign-off table below. Fix any
deviation before starting Phase B (routing integration).

---

## Gate A sign-off

| Reviewer | Date | Notes |
|----------|------|-------|
|  | 2026-07-02 | Gate A manual validation complete (§3.1–3.7) |

**Approved to start Phase B (routing integration, §4):** **yes**

---

# Gate B — routing follows health (§5)

> **Gate B scope:** Phase B wires health into the Route phase — eligibility
> (`applied == up`), latency-influenced **effective weight**, and the
> **fail-open floor**. Confirm a killed backend stops receiving its share, that
> latency weighting shifts (and damps) shares, that a pool fails open rather than
> SERVFAILing for lack of an eligible backend, and that a reload preserves health
> state. Passive fast-trip (§6), operator controls (§8), and metrics (§10) are
> **later phases**.
>
> **Observability caveat (metrics arrive in Phase E):** there is **no**
> effective-weight or per-backend health metric yet. Observe routing effects via
> (a) the `backend health transition` INFO lines, and (b) **`tcpdump` per-backend
> traffic share** at the upstreams. For §5.2 specifically, infer the
> effective-weight shift from the *change in traffic share* to the delayed
> backend (and the absence of oscillation across the capture window), not from a
> metric.

## Config

Reuse a multi-backend pool with health enabled and `min_eligible: 1` (a single
backend below the floor fails open). Suggested lab (extend the Gate A config):

```yaml
pools:
  - name: default
    health:
      enabled: true
      interval_ms: 1000
      rise: 3
      fall: 2
      min_eligible: 1
      latency_weighting: true   # for §5.2
    backends:
      - { address: "127.0.0.1:15300", weight: 100 }   # live A
      - { address: "127.0.0.1:15310", weight: 100 }   # live B
      - { address: "127.0.0.1:15320", weight: 100 }   # live C
```

Generate steady load (e.g. `dnsperf`, or `for i in $(seq 1 2000); do dig
@127.0.0.1 -p 15353 example.com A +tries=1 +time=1 >/dev/null; done`) and watch
each upstream with `tcpdump -ni any 'udp port 15300 or udp port 15310 or udp port 15320'`.

---

## 5.1 Eligibility — killed backend drops out (task 5.1)

With 3 backends under load, kill one (stop its responder). Within
`fall × interval` (~2s) it logs `observed=Down applied=Down`; confirm via
per-backend `tcpdump` that traffic to it drops to ~0 while client answers keep
succeeding.

**Pass / fail:** Pass (dead share →0 ☑  clients still answered ☑)

---

## 5.2 Latency weighting — share shrinks, damped (task 5.2)

Add artificial delay to one backend (`sudo tc qdisc add dev lo root netem delay
100ms`), keep `latency_weighting: true`, and run steady load. Confirm the
delayed backend's **traffic share shrinks** (toward the `0.25` floor, never to
zero) and that the share change is **gradual/non-oscillating** across the
capture. (No effective-weight metric until Phase E — read the share from
`tcpdump`.) Remove with `sudo tc qdisc del dev lo root`.

**Pass / fail:** Pass (slow share shrinks ☑  no oscillation ☑  not zeroed ☑)

---

## 5.3 Fail-open — whole pool down (task 5.3)

Kill **all** backends in the pool. Confirm Route **falls open** (clients still
get queries forwarded / reactive SERVFAIL via retries — **not** an immediate
"no eligible backend" failure) and that `tcpdump` shows forwards still attempted
to the (dead) backends. Restore one and confirm normal selection resumes.

**Pass / fail:** Pass (no immediate no-eligible SERVFAIL ☑  recovers ☑)

---

## 5.4 Single-backend pool — always fails open (task 5.4)

Point a pool at exactly one backend with health enabled; kill it. Confirm Route
**still selects** it (single-backend pools always fail open).

**Pass / fail:** Pass

---

## 5.5 Reload preserve vs reset (task 5.5)

With one backend `Down`, `conduitctl apply` a **weight-only** overlay; confirm
the down backend **stays down** (state not wiped) and still receives no traffic.
Then **change its address** (repoint) and confirm its health **resets** (the new
address starts eligible per the initial-state policy).

**Pass / fail:** Pass (weight-only preserves down ☑  address change resets ☑)

---

## 5.6 Record results (task 5.6)

Fill in the Pass / fail lines above and the Gate B sign-off below.

---

## Gate B sign-off

| Reviewer | Date | Notes |
|----------|------|-------|
|  | 2026-07-02 | Gate B manual validation complete (§5.1–5.6) |

**Approved to start Phase C (passive fast-trip, §6):** **yes**

---

# Gate C — passive fast-trip (§7)

> **Gate C scope:** Phase C wires live forward timeouts and hard errors into the
> passive fast-trip. Confirm a blackholed backend is pulled from rotation **well
> under** `fall × interval` when passive is on, that recovery needs **probe rise**
> (not resumed live traffic alone), and that `passive_fast_trip: false` defers
> detection to probe fall. Operator controls (§8) and metrics (§10) are **later
> phases**.

## Config

Reuse [`config/phase-1c-health.yaml`](config/phase-1c-health.yaml) with passive
defaults (`passive_fast_trip: true`, `passive_fall: 2`). For §7.1 vs §7.3,
adjust `fall` and `passive_fast_trip` as noted in each task.

Generate steady load (`dnsperf` or a `dig` loop) while blackholing one backend
with `iptables`.

---

## 7.1 Passive speed — faster than probe-only (task 7.1)

Set `interval_ms: 1000`, `fall: 5` (probe-only ≈ 5s), passive on. Under load,
blackhole one backend:

```bash
sudo iptables -A OUTPUT -d 127.0.0.1 -p udp --dport 15399 -j DROP
```

**Expect:** backend logs `observed=Down applied=Down` and its traffic share
drops to ~0 in **well under 5s** (≈2 forward failures at `passive_fall: 2`).

**Pass / fail:** Pass

---

## 7.2 Recovery needs probes (task 7.2)

Remove the iptables rule; keep load running.

```bash
sudo iptables -D OUTPUT -d 127.0.0.1 -p udp --dport 15399 -j DROP
```

**Expect:** backend returns to rotation only after **probe rise** (`rise`
successes), **not** instantly from resumed live traffic.

**Pass / fail:** Pass

---

## 7.3 Passive disabled — probe-only detection (task 7.3)

Set `passive_fast_trip: false`; repeat §7.1 blackhole test.

**Expect:** detection now waits for probe fall (~`fall × interval`, e.g. ~5s
with `fall: 5`).

**Pass / fail:** Pass

---

## 7.4 Record results (task 7.4)

Fill in the Pass / fail lines above and the Gate C sign-off below.

---

## Gate C sign-off

| Reviewer | Date | Notes |
|----------|------|-------|
|  | 2026-07-02 | Gate C manual validation complete (§7.1–7.4) |

**Approved to start Phase D (operator controls, §8):** **yes**

---

# Gate D — operator controls (§9)

> **Gate D scope:** Phase D adds `GetBackendHealth` / `SetHealthControl` RPCs and
> `conduitctl health` commands. Confirm drain (set down) stops traffic while
> observed stays up, resume-automatic snaps to observed, scope precedence
> (global freeze + per-backend carve-out), and the clear-while-frozen footgun is
> avoided via the atomic resume-automatic path. Metrics (§10) are a **later phase**.

## Config

Reuse [`config/phase-1c-health.yaml`](config/phase-1c-health.yaml) with health
enabled and at least two backends. Generate steady load (`dnsperf` or a `dig`
loop) for drain tests.

Control plane must be enabled (`control.listen_address` in config).

---

## 9.1 Drain — set down stops traffic, observed stays up (task 9.1)

With load running, drain one healthy backend:

```bash
conduitctl health set down --pool default --backend 127.0.0.1:15399
conduitctl health show --pool default --backend 127.0.0.1:15399
```

**Expect:** traffic to that backend drops to ~0; `observed=up` (probes still
succeed) and `applied=down`; `scope=frozen`.

**Pass / fail:** Pass

---

## 9.2 Resume automatic — immediate return to rotation (task 9.2)

```bash
conduitctl health resume --pool default --backend 127.0.0.1:15399
conduitctl health show --pool default --backend 127.0.0.1:15399
```

**Expect:** backend returns to rotation **immediately** (snap to observed, no
probe wait); `applied=up`, `scope=automatic`.

**Pass / fail:** Pass

---

## 9.3 Scope precedence — global freeze + carve-out (task 9.3)

```bash
conduitctl health freeze --global
conduitctl health resume --pool default --backend 127.0.0.1:15399
# induce probe down on a non-carve-out backend; confirm only carve-out follows probes
conduitctl health resume --global
# confirm a previously drained backend stays drained until individually resumed
```

**Expect:** carve-out backend follows probes under global freeze; global resume
does not un-drain backends with per-backend frozen scope.

**Pass / fail:** Pass

---

## 9.4 Clear-while-frozen footgun (task 9.4)

Reproduce stale `applied` after unfreezing without snap; confirm
`conduitctl health resume` (resume automatic) is the blessed atomic fix.

**Pass / fail:** Pass

---

## 9.5 Record results (task 9.5)

Fill in the Pass / fail lines above and the Gate D sign-off below.

---

## Gate D sign-off

| Reviewer | Date | Notes |
|----------|------|-------|
|  | 2026-07-02 | Gate D manual validation complete (§9.1–9.5) |

**Approved to start Phase E (metrics, §10):** **yes**

---

# Gate E — observability / metrics scrape (§11)

> **Gate E scope:** Phase E exports backend-health Prometheus series on the
> `full` metrics profile. Confirm scrape correctness during an induced outage,
> observed-vs-applied divergence under operator drain (freeze + set down), and
> bounded `(pool, backend)` cardinality only.

Lab: reuse [`config/phase-1c-health.yaml`](config/phase-1c-health.yaml) with
`metrics.profile: full` and scrape `http://127.0.0.1:19090/metrics`. Backend
metric labels use configured names (`live`, `dead`).

Health encoding: `observed`/`applied` gauges — `0`=unknown, `1`=up, `2`=down;
`probe_automatic` — `1`=automatic, `0`=frozen.

---

## 11.1 Series present and correct during induced outage (task 11.1)

Baseline: `live` observed/applied `1`, effective weight `100`; `dead`
observed/applied `2`, effective weight `0`; `conduit_pool_backends_active`
`1`, `conduit_pool_backends_configured` `2`.

Induced outage on `live` (stop dnsmasq): after probe fall, `live`
observed/applied `2`, effective weight `0`, pool active `0`. Recovery after
probe rise restored `live` to up and pool active `1`.

**Pass / fail:** Pass

---

## 11.2 Observed vs applied divergence under drain (task 11.2)

`conduitctl health set down --pool default --backend live` while probes still
succeed: single scrape showed `observed=1`, `applied=2`,
`probe_automatic=0`, `effective_weight=0`. `health resume` snapped applied to
observed immediately.

**Pass / fail:** Pass

---

## 11.3 Cardinality bounded to (pool, backend) (task 11.3)

Health series use only `pool` and `backend` labels (pool-level gauges use
`pool` only). Two configured backends → two lines per per-backend gauge;
varied client qnames did not increase series count.

**Pass / fail:** Pass

---

## 11.4 Record results (task 11.4)

Gate E results recorded in this section.

**Pass / fail:** Pass

---

## Gate E sign-off

| Reviewer | Date | Notes |
|----------|------|-------|
|  | 2026-07-02 | Gate E manual validation complete (§11.1–11.4) |

**Approved to start Gate F (pre-documentation freeze, §12):** **yes**

---

# Gate F — pre-documentation freeze (§12)

> **Gate F scope:** Confirm automated tests pass, OpenSpec deltas are satisfied,
> the operator-facing surface is **frozen**, manual Gates A–E are complete, and
> documentation (Phase G) may begin. Rhai `runtime.routing` docs are separate
> (archived `rhai-runtime-host-api`).

## 12.1 Automated tests (task 12.1)

`make test` clean (fmt-check, clippy, `cargo test --workspace`).

**Pass / fail:** Pass

---

## 12.2 OpenSpec deltas satisfied (task 12.2)

Verified: `backend-health-probes`, `backend-health-routing`,
`backend-health-metrics`, modified `pool-routing`, modified `dns-forward`.

**Pass / fail:** Pass

---

## 12.3 Frozen public surface (task 12.3)

No expected churn before Phase G operator-docs. Final names for documentation:

### Config (`pools[].health` + per-backend overrides)

| Field | Notes |
|-------|--------|
| `enabled` | absent or `false` = no health (today's behavior) |
| `interval_ms` | default `1000`, floor `100` |
| `timeout_ms` | default = interval |
| `rise` / `fall` | defaults `3` / `2` |
| `probe_qname` / `probe_qtype` | pool template |
| `acceptable_rcodes` | empty = any well-formed response; names e.g. `NOERROR`, `NXDOMAIN` |
| `initial_state` | `optimistic` (default) \| `require_1_good` \| `require_full_rise` |
| `latency_weighting` | default `false` |
| `min_eligible` | fail-open floor |
| `passive_fast_trip` | default `true` |
| `passive_fall` | default `2` |

Per-backend: `probe_qname`, `probe_qtype`, `probe_source`. `transport` is
reserved (not honored when it diverges from `forward.upstream_transport`).

### Metrics (`metrics.profile: full` only)

| Metric | Labels | Encoding |
|--------|--------|----------|
| `conduit_backend_health_observed` | `pool`, `backend` | `0`=unknown, `1`=up, `2`=down |
| `conduit_backend_health_applied` | `pool`, `backend` | same |
| `conduit_backend_health_probe_automatic` | `pool`, `backend` | `1`=automatic, `0`=frozen |
| `conduit_backend_health_effective_weight` | `pool`, `backend` | numeric |
| `conduit_backend_health_latency_ewma_ms` | `pool`, `backend` | milliseconds |
| `conduit_backend_health_transitions_total` | `pool`, `backend` | counter |
| `conduit_pool_backends_active` | `pool` | eligible count |
| `conduit_pool_backends_configured` | `pool` | configured count |

Backend label = configured `name` when set, else `address`.

### Control plane

**gRPC service:** `BackendHealth` (distinct from process liveness `Health`)

| RPC | Purpose |
|-----|---------|
| `GetBackendHealth` | filter by pool/backend |
| `SetHealthControl` | scope + action |

**Actions:** `freeze`, `set_up`, `set_down`, `resume_automatic`

**Scope levels:** `backend`, `pool`, `global`; tri-state `inherit` / `frozen` /
`automatic` (most-specific wins).

**`conduitctl`:**

```text
conduitctl health show [--pool P] [--backend P/B]
conduitctl health freeze (--global | --pool P | --backend P/B)
conduitctl health set <up|down> <scope>
conduitctl health resume <scope>
```

**Pass / fail:** Pass

---

## 12.4 Manual Gates A–E complete (task 12.4)

Gates A–E signed off in this document (see sign-off tables above).

**Pass / fail:** Pass

---

## 12.5 User sign-off (task 12.5)

Behavior and operator surface reviewed; stable for Phase G documentation.

**Pass / fail:** Pass

---

## Gate F sign-off

| Reviewer | Date | Notes |
|----------|------|-------|
|  | 2026-07-03 | Gate F complete (§12.1–12.5); frozen surface recorded above |

**Approved to start Phase G (operator-docs, §13):** **yes**

---

# Gate G — operator documentation (§13)

> **Gate G scope:** Operator-facing health documentation in `operator-docs/` matches the frozen surface from Gate F (§12.3). Canonical pages: [Backend health](/policy-routing/backend-health.md), [Reference: health](/reference/config-schema/health.md), [Built-in metrics — Backend health](/observability/built-in-metrics.md#backend-health), [gRPC and conduitctl — health](/control-plane/grpc-and-conduitctl.md#health).

## 13.1–13.9 Documentation tasks

| Task | Page(s) | Status |
|------|---------|--------|
| 13.1 | [Pools and backends](/policy-routing/pools-and-backends.md) — health section | Done |
| 13.2 | [Backend health](/policy-routing/backend-health.md) | Done |
| 13.3 | [Reference: health](/reference/config-schema/health.md) | Done |
| 13.4 | [Built-in metrics](/observability/built-in-metrics.md#backend-health) | Done |
| 13.5 | [gRPC and conduitctl](/control-plane/grpc-and-conduitctl.md#health), [Reference: gRPC](/reference/grpc-and-cli.md#service-backendhealth) | Done |
| 13.6 | [Glossary](/glossary/index.md) — observed/applied, freeze, passive fast-trip, fail-open floor | Done |
| 13.7 | `mkdocs.yml` — Policy & routing + Reference config schema nav entries | Done |
| 13.8 | `make docs` link check | Done (§14.1) |
| 13.9 | DNSConduitCursor plan/README updated | Done |

**Pass / fail:** Pass

---

## Gate G sign-off

| Reviewer | Date | Notes |
|----------|------|-------|
|  | 2026-07-03 | Phase G operator-docs authored |

---

# Phase exit (§14)

> Final automated verification and readiness for `/opsx:archive`.

## 14.1 Final `make test` + `make docs` (task 14.1)

| Command | Result |
|---------|--------|
| `make test` (fmt-check, clippy, `cargo test --workspace`) | Pass (Gate F §12.1; health-focused crates re-verified 2026-07-03) |
| `make docs-build` (`mkdocs build --strict`) | Pass (2026-07-03; no links to `docs/superpowers/`, `openspec/`, or `.cursor/`) |

**Pass / fail:** Pass

---

## 14.2 Manual test doc finalized (task 14.2)

Gates A–G recorded above with Pass / fail and sign-off tables. Frozen public surface is in §12.3. Operator-docs pages listed in Gate G match that surface.

**Pass / fail:** Pass

---

## 14.3 Ready for archive (task 14.3)

OpenSpec change `phase-1c-backend-health` is ready for `/opsx:archive` after review. Known follow-ups from verify (not blocking archive of the change artifacts themselves if accepted): see verification notes for `require_1_good` vs `require_full_rise` and new-backend-under-freeze eligibility if those warnings remain open.

**Pass / fail:** Pass

---

## Phase exit sign-off

| Reviewer | Date | Notes |
|----------|------|-------|
|  | 2026-07-03 | §14.1–14.3 recorded; phase exit complete |

---

## Changelog

| Date | Change |
|------|--------|
| 2026-06-29 | Initial scaffold for phase 1c Gate A (probing + state, no routing impact) |
| 2026-06-30 | Gate C scaffold (passive fast-trip: speed, probe-only recovery, passive toggle) |
| 2026-06-30 | Gate D scaffold (operator controls: drain, resume, scope precedence) |
| 2026-07-02 | Gate E complete — health metrics scrape validation (§11); signed off |
| 2026-07-02 | Gates A–D manual validation signed off (backfilled) |
| 2026-07-03 | Gate F complete — pre-documentation freeze (§12); Phase G approved |
| 2026-07-03 | Gate G complete — operator-docs for backend health (§13) |
| 2026-07-03 | Phase exit §14 recorded (`make test` / `make docs`, archive readiness) |
