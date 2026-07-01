# Manual test guide — phase 1c backend health (Gates A–B)

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

**Pass / fail:** ___________

---

## 1. Two-backend pool, health enabled (task 3.1)

Confirm the lab is wired: pool `default` has `live` (15300) and `dead` (15399),
health enabled at `interval_ms: 1000`.

**Verify (Terminal C):**

```bash
rg -n 'backends=2' <conduit stdout>           # probe loop covers both backends
```

**Expect:** probe loop reports `backends=2`; Conduit is serving on `127.0.0.1:15353`.

**Pass / fail:** ___________

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

**Pass / fail:** ___________ (cadence ≈1s ☐  qname/qtype correct ☐  qids vary ☐)

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

**Pass / fail:** ___________ (dead→down ☐  live kill→down ☐  live restore→up ☐)

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

**Pass / fail:** ___________

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

**Pass / fail:** ___________

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

**Pass / fail:** ___________

---

## 7. Record results (task 3.7)

Fill in the Pass / fail lines above and the sign-off table below. Fix any
deviation before starting Phase B (routing integration).

---

## Gate A sign-off

| Reviewer | Date | Notes |
|----------|------|-------|
|  |  |  |

**Approved to start Phase B (routing integration, §4):** **___**

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

**Pass / fail:** ___________ (dead share →0 ☐  clients still answered ☐)

---

## 5.2 Latency weighting — share shrinks, damped (task 5.2)

Add artificial delay to one backend (`sudo tc qdisc add dev lo root netem delay
100ms`), keep `latency_weighting: true`, and run steady load. Confirm the
delayed backend's **traffic share shrinks** (toward the `0.25` floor, never to
zero) and that the share change is **gradual/non-oscillating** across the
capture. (No effective-weight metric until Phase E — read the share from
`tcpdump`.) Remove with `sudo tc qdisc del dev lo root`.

**Pass / fail:** ___________ (slow share shrinks ☐  no oscillation ☐  not zeroed ☐)

---

## 5.3 Fail-open — whole pool down (task 5.3)

Kill **all** backends in the pool. Confirm Route **falls open** (clients still
get queries forwarded / reactive SERVFAIL via retries — **not** an immediate
"no eligible backend" failure) and that `tcpdump` shows forwards still attempted
to the (dead) backends. Restore one and confirm normal selection resumes.

**Pass / fail:** ___________ (no immediate no-eligible SERVFAIL ☐  recovers ☐)

---

## 5.4 Single-backend pool — always fails open (task 5.4)

Point a pool at exactly one backend with health enabled; kill it. Confirm Route
**still selects** it (single-backend pools always fail open).

**Pass / fail:** ___________

---

## 5.5 Reload preserve vs reset (task 5.5)

With one backend `Down`, `conduitctl apply` a **weight-only** overlay; confirm
the down backend **stays down** (state not wiped) and still receives no traffic.
Then **change its address** (repoint) and confirm its health **resets** (the new
address starts eligible per the initial-state policy).

**Pass / fail:** ___________ (weight-only preserves down ☐  address change resets ☐)

---

## 5.6 Record results (task 5.6)

Fill in the Pass / fail lines above and the Gate B sign-off below.

---

## Gate B sign-off

| Reviewer | Date | Notes |
|----------|------|-------|
|  |  |  |

**Approved to start Phase C (passive fast-trip, §6):** **___**

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

**Pass / fail:** ___________

---

## 7.2 Recovery needs probes (task 7.2)

Remove the iptables rule; keep load running.

```bash
sudo iptables -D OUTPUT -d 127.0.0.1 -p udp --dport 15399 -j DROP
```

**Expect:** backend returns to rotation only after **probe rise** (`rise`
successes), **not** instantly from resumed live traffic.

**Pass / fail:** ___________

---

## 7.3 Passive disabled — probe-only detection (task 7.3)

Set `passive_fast_trip: false`; repeat §7.1 blackhole test.

**Expect:** detection now waits for probe fall (~`fall × interval`, e.g. ~5s
with `fall: 5`).

**Pass / fail:** ___________

---

## 7.4 Record results (task 7.4)

Fill in the Pass / fail lines above and the Gate C sign-off below.

---

## Gate C sign-off

| Reviewer | Date | Notes |
|----------|------|-------|
|  |  |  |

**Approved to start Phase D (operator controls, §8):** **___**

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

**Pass / fail:** ___________

---

## 9.2 Resume automatic — immediate return to rotation (task 9.2)

```bash
conduitctl health resume --pool default --backend 127.0.0.1:15399
conduitctl health show --pool default --backend 127.0.0.1:15399
```

**Expect:** backend returns to rotation **immediately** (snap to observed, no
probe wait); `applied=up`, `scope=automatic`.

**Pass / fail:** ___________

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

**Pass / fail:** ___________

---

## 9.4 Clear-while-frozen footgun (task 9.4)

Reproduce stale `applied` after unfreezing without snap; confirm
`conduitctl health resume` (resume automatic) is the blessed atomic fix.

**Pass / fail:** ___________

---

## 9.5 Record results (task 9.5)

Fill in the Pass / fail lines above and the Gate D sign-off below.

---

## Gate D sign-off

| Reviewer | Date | Notes |
|----------|------|-------|
|  |  |  |

**Approved to start Phase E (metrics, §10):** **___**

---

## Changelog

| Date | Change |
|------|--------|
| 2026-06-29 | Initial scaffold for phase 1c Gate A (probing + state, no routing impact) |
| 2026-06-30 | Gate C scaffold (passive fast-trip: speed, probe-only recovery, passive toggle) |
| 2026-06-30 | Gate D scaffold (operator controls: drain, resume, scope precedence) |
