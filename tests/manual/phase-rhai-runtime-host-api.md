# Manual test guide — Rhai runtime host API

> **OpenSpec:** `rhai-runtime-host-api`. Exercises breaking Rhai API changes (`lookup`,
> `metrics`, `runtime.routing`) and health-aware script reads. Complements the phase **1c**
> health lab ([`phase-1c-backend-health.md`](phase-1c-backend-health.md)).

All commands assume your shell is at the DNSConduit repo root.

## Port map

| Role | Address |
|------|---------|
| Conduit DNS (UDP) | `127.0.0.1:15353` |
| Pool backend **live** | `127.0.0.1:15300` |
| Pool backend **second / failover** | `127.0.0.1:15399` |
| Control gRPC | `127.0.0.1:5199` |

## Prerequisites

```bash
cargo build -p conduit -p conduitctl --release
```

Tools: `dig`, `dnsmasq` (for labs that need a real answer).

---

## Lab 1 — New API smoke: blocklist (`lookup` + `metrics.inc` + drop)

**Config:** [`tests/fixtures/config/with-rhai-blocklist.yaml`](../fixtures/config/with-rhai-blocklist.yaml)  
**Script:** [`tests/fixtures/rhai/blocklist.rhai`](../fixtures/rhai/blocklist.rhai)

| Terminal | Role |
|----------|------|
| **B** | Conduit |
| **C** | `dig` / logs |

**B — start Conduit:**

```bash
RUST_LOG=info ./target/release/conduit tests/fixtures/config/with-rhai-blocklist.yaml
```

**C — blocked name (policy drop, no reply):**

```bash
dig @127.0.0.1 -p 15353 bad.example. A +time=2 +tries=1
```

**Expect:** timeout / no answer (silent drop). No Rhai eval errors in logs.

**C — validate:**

```bash
./target/release/conduitctl validate --file tests/fixtures/config/with-rhai-blocklist.yaml
```

**Expect:** validation succeeds.

---

## Lab 2 — Breaking change: old API fails validate

**C — throwaway script with legacy API:**

```bash
cat > /tmp/old-api.rhai <<'EOF'
if table_lookup("blocklist", question_qname(txn)) == "block" {
    txn.metric_inc("block_hits", 1);
    txn.drop_query();
}
EOF
```

Point a minimal config `rules` action at `/tmp/old-api.rhai`, then:

```bash
./target/release/conduitctl validate --file <that-config.yaml>
```

**Expect:** compile/validation failure (legacy symbols removed).

---

## Lab 3 — `runtime.routing().pool()` + health drain → pool switch

**Config:** [`tests/fixtures/config/with-rhai-routing-pool.yaml`](../fixtures/config/with-rhai-routing-pool.yaml)  
**Script:** [`tests/fixtures/rhai/routing-pool-failover.rhai`](../fixtures/rhai/routing-pool-failover.rhai)

Pool **`primary`** backends: `127.0.0.1:15300`, `127.0.0.1:15399`. Pool **`secondary`**: `127.0.0.1:15399`.

| Terminal | Role |
|----------|------|
| **A** | dnsmasq on **15399** (failover answers) |
| **B** | Conduit |
| **C** | `dig`, `conduitctl health` |

**A — upstream on the failover backend:**

```bash
dnsmasq --keep-in-foreground --port=15399 --bind-interfaces \
  --listen-address=127.0.0.1 --no-resolv --address=/test.example/192.0.2.50
```

Leave **15300** without a listener (or stop dnsmasq there) so draining it is meaningful.

**B — start Conduit:**

```bash
RUST_LOG=info ./target/release/conduit tests/fixtures/config/with-rhai-routing-pool.yaml
```

**C — drain the live primary backend:**

```bash
./target/release/conduitctl health set down --pool primary --backend 127.0.0.1:15300
./target/release/conduitctl health show --pool primary --backend 127.0.0.1:15300
```

**Expect:** `applied=down` (or equivalent).

**C — query:**

```bash
dig @127.0.0.1 -p 15353 test.example. A +short
```

**Expect:** **192.0.2.50** via pool **`secondary`** / backend **`127.0.0.1:15399`**. Request-hook script switches pool when `eligible_count < configured_count`.

**Cleanup:**

```bash
./target/release/conduitctl health resume --pool primary --backend 127.0.0.1:15300
```

---

## Lab 4 — `runtime.routing().backend_for_attempt()` + retry (response hook)

**Config:** [`tests/fixtures/config/with-rhai-routing-backend.yaml`](../fixtures/config/with-rhai-routing-backend.yaml)  
**Script:** [`tests/fixtures/rhai/routing-backend-attempt.rhai`](../fixtures/rhai/routing-backend-attempt.rhai)

Same backend ports as Lab 3. Full end-to-end needs SERVFAIL from primary forward and a drained attempt backend — mirror [`with-rhai-servfail-retry.yaml`](../fixtures/config/with-rhai-servfail-retry.yaml) with health enabled.

**Automated check (CI):**

```bash
cargo test -p conduit-core --test rhai_routing_health
```

---

## Lab 5 — Lookup demo (`txn.question().qname`)

```bash
RUST_LOG=info ./target/release/conduit tests/fixtures/config/with-rhai-lookup-demo.yaml
dig @127.0.0.1 -p 15353 <qname-from-geo-csv> A
```

**Expect:** `lookup("geo", txn.question().qname)` reads the geo table (see `lookup-demo.rhai`).
