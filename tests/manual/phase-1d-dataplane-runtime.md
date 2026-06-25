# Manual test guide — phase 1d dataplane runtime models

> **Repository:** DNSConduit root. **OpenSpec:** `dataplane-runtime-models`.  
> **Execution plan:** `docs/superpowers/plans/2026-06-23-dataplane-runtime-models-execution.md` (DNSConduitCursor).  
> **Gate A:** Complete sections **1–8** (and optional **9**) before operator documentation (Task 12.x).

## Port map

| Role | Address |
|------|---------|
| Conduit DNS (UDP) | `127.0.0.1:15353` |
| Conduit DNS (TCP, optional) | `127.0.0.1:15354` |
| Upstream mock (dnsmasq) | `127.0.0.1:15300` → `$UPSTREAM_DNS` |
| Prometheus scrape | `http://127.0.0.1:19090/metrics` |
| Control gRPC | `127.0.0.1:5199` |

## Configs

| Purpose | File |
|---------|------|
| sync baseline (no `dataplane:`) | [`config/phase-1d-sync-default.yaml`](config/phase-1d-sync-default.yaml) |
| split_io production-style | [`config/phase-1d-split-io.yaml`](config/phase-1d-split-io.yaml) |
| slow upstream concurrency lab | [`config/phase-1d-split-io-slow-upstream.yaml`](config/phase-1d-split-io-slow-upstream.yaml) |
| named backends + metrics | [`config/phase-1d-split-io-named-backends.yaml`](config/phase-1d-split-io-named-backends.yaml) |
| per-listener overrides | [`config/phase-1d-per-listener.yaml`](config/phase-1d-per-listener.yaml) |
| invalid runtime (validate only) | [`config/phase-1d-invalid-runtime.yaml`](config/phase-1d-invalid-runtime.yaml) |

Related: [`phase-4b-slow-upstream.yaml`](config/phase-4b-slow-upstream.yaml), [`phase-4b-operator-metrics.md`](phase-4b-operator-metrics.md).

## Prerequisites

```bash
cd /path/to/DNSConduit
cargo build -p conduit --release
export UPSTREAM_DNS=8.8.8.8
chmod +x tests/manual/scripts/check-ports.sh
tests/manual/scripts/check-ports.sh
```

Tools: `dig`, `curl`, `rg`, `dnsmasq`, optional `grpcurl`, `conduitctl`.

## Terminal layout

| Terminal | Role |
|----------|------|
| **A** | dnsmasq on `15300` |
| **B** | Conduit |
| **C** | traffic, scrape, overlay |

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

## 1. Sync default regression

**Config:** `phase-1d-sync-default.yaml` (no `dataplane:` block).

**Terminal B:**

```bash
cargo run -p conduit -- tests/manual/config/phase-1d-sync-default.yaml
```

**Terminal C:**

```bash
dig @127.0.0.1 -p 15353 +time=3 +tries=1 www.example.com A
```

**Expect:** NOERROR; behavior indistinguishable from pre-1d (latency, retries, logs at default level). Run `make test` before this section in CI.

**Pass / fail:** ___________

---

## 2. split_io basic forward

**Config:** `phase-1d-split-io.yaml`.

Restart Conduit with split_io config. Single `dig` as above.

**Expect:** NOERROR; startup log mentions split_io / policy workers (exact wording TBD at implementation).

**Pass / fail:** ___________

---

## 3. split_io concurrency (core acceptance)

**Config:** `phase-1d-split-io-slow-upstream.yaml` (`timeout_ms: 10000` or block upstream briefly).

**Procedure:**

1. Start Conduit with slow-upstream config.
2. Terminal C — query A (blocks on slow upstream):

   ```bash
   dig @127.0.0.1 -p 15353 +time=12 +tries=1 slow-test.example.com A
   ```

3. **While A is in flight**, query B:

   ```bash
   dig @127.0.0.1 -p 15353 +time=3 +tries=1 www.example.com A
   ```

**Expect:**

- **split_io:** B completes without waiting for A's full upstream wait (ingress not blocked).
- **sync** (repeat with `phase-1d-sync-default.yaml` + same timeout): B may wait behind A on same worker — document observed difference.

**Pass / fail:** ___________

---

## 4. Slot and forward_outstanding metrics

**Config:** slow-upstream + `metrics.prometheus` enabled.

During delayed forward (section 3):

```bash
curl -sS http://127.0.0.1:19090/metrics | rg 'conduit_slots_|conduit_forward_outstanding'
```

**Expect:**

- `conduit_forward_outstanding{...}` ≥ 1 while upstream pending
- `conduit_slots_in_use` ≥ 1 during parked wait (split_io)
- Gauges return toward zero after queries complete

**Pass / fail:** ___________

---

## 5. Named backend metrics label

**Config:** `phase-1d-split-io-named-backends.yaml` (backends with `name:`).

After several queries:

```bash
curl -sS http://127.0.0.1:19090/metrics | rg 'conduit_forward_attempts_total|conduit_forward_duration'
```

**Expect:** `backend="resolver-east"` (or configured name), not only `127.0.0.1:15300`.

**Pass / fail:** ___________

---

## 6. Per-listener overrides

**Config:** `phase-1d-per-listener.yaml` (e.g. UDP `:15353` many threads + `reuse_port`; TCP `:15354` single thread).

**Expect:** Process thread count reflects config; both protocols answer `dig` on respective ports.

**Pass / fail:** ___________

---

## 7. Overlay patch by backend name

**Config:** `phase-1d-split-io-named-backends.yaml` + control plane enabled.

Create `overlay-weight.yaml`:

```yaml
schema_version: 1
pools:
  - name: default
    backends:
      - name: resolver-east
        weight: 10
```

```bash
conduitctl apply --file overlay-weight.yaml
conduitctl export | rg -A5 'backends:'
```

**Expect:** Effective weight 10 without repeating `address` in patch. Unknown `name` in overlay → apply rejected.

**Pass / fail:** ___________

---

## 8. TCP client under split_io

**Config:** split_io with TCP listener on `127.0.0.1:15354`.

```bash
dig @127.0.0.1 -p 15354 +tcp +time=3 www.example.com A
```

**Expect:** NOERROR; no hang on connection owner / reply path.

**Pass / fail:** ___________

---

## 9. Drain API smoke (optional)

*Only when `DataplaneHandle::drain` is exposed for manual testing (debug hook or short-lived test binary).*

1. Start split_io with slow upstream; launch in-flight query.
2. Trigger drain (API TBD) with timeout.
3. Close listeners; verify in-flight query still completes or times out cleanly.
4. Repeat with `DrainFilter` UDP-only if two protocols configured — TCP slot not required for UDP drain completion.

**Pass / fail:** ___________ / N/A

---

## Gate A sign-off

| Reviewer | Date | Notes |
|----------|------|-------|
| | | |

**Approved to start operator documentation (Task 12.x):** yes / no

---

## Changelog

| Date | Change |
|------|--------|
| 2026-06-23 | Initial scaffold for phase 1d Gate A |
