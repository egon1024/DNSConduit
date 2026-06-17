# Manual test guide — ordered rule actions

> **Repository:** DNSConduit root. Exercises **list-order** rule execution, the **drop** family, request **`set_retry_pool`**, and **`rhai`** interleaved with built-ins.
>
> Operator reference: [Rules and actions — Action order](/operator-docs/docs/policy-routing/rules-and-actions.md#action-order-on-one-rule).

## Port map

| Role | Address |
|------|---------|
| Conduit DNS (UDP) | `127.0.0.1:15353` |
| Upstream **default** pool | `127.0.0.1:15300` |
| Upstream **backup** pool (optional) | `127.0.0.1:15301` |

## Config

[`config/09-ordered-actions.yaml`](config/09-ordered-actions.yaml)

## Prerequisites

```bash
cd /path/to/DNSConduit
cargo build -p conduit -p conduitctl --release
tests/manual/scripts/check-ports.sh
```

**Terminal A** — default upstream:

```bash
dnsmasq --port=15300 --bind-interfaces --listen-address=127.0.0.1 \
  --no-resolv --address=/manual-order.example/192.0.2.10
```

**Terminal B** — Conduit:

```bash
target/release/conduit tests/manual/config/09-ordered-actions.yaml
```

**Terminal C** — queries:

```bash
dig @127.0.0.1 -p 15353 +time=2 +tries=1 +tcp +ignore manual-order.example A
```

Baseline **expect:** `192.0.2.10` (normal forward through **default** pool).

---

## 1. Soft `drop` — later actions still run

```bash
dig @127.0.0.1 -p 15353 +time=2 +tries=1 +tcp +ignore soft-drop.manual-order.example A
```

**Expect:** no answer (timeout / no response). The rule sets `drop` then `set_tag: audited=true`; the tag step still runs even though the query is dropped at the end of the rule.

---

## 2. `drop_now` — short-circuit

```bash
dig @127.0.0.1 -p 15353 +time=2 +tries=1 +tcp +ignore hard-drop.manual-order.example A
```

**Expect:** no answer. `set_tag: skipped=true` does **not** run (hard drop stops the action list).

---

## 3. `clear_drop` — cancels soft drop

```bash
dig @127.0.0.1 -p 15353 +time=2 +tries=1 +tcp +ignore clear-drop.manual-order.example A
```

**Expect:** `192.0.2.10` — soft drop is cleared before the rule ends, so the query forwards normally.

---

## 4. Request `set_retry_pool` — pool for retry if retry occurs; first Route ignores it

```bash
dig @127.0.0.1 -p 15353 +time=2 +tries=1 +tcp +ignore retry-stash.manual-order.example A
```

**Expect:** `192.0.2.10` from **default** (`127.0.0.1:15300`), not backup. Request policy sets `retry_pool: backup` for use if a later retry occurs; the first forward still uses `selected_pool: default`.

**Full retry failover** (response triggers retry, backup pool used): see fixture cookbook example 3 — [`with-rhai-servfail-retry.yaml`](../fixtures/config/with-rhai-servfail-retry.yaml) and [`servfail-retry.rhai`](../fixtures/rhai/servfail-retry.rhai) in [`tests/fixtures/rhai/README.md`](../fixtures/rhai/README.md).

---

## 5. Action list order — `rhai` before `set_pool`

Rule `rhai-order-demo` runs [`set-vip-pool.rhai`](../fixtures/rhai/set-vip-pool.rhai) **then** `set_pool: default`.

```bash
dig @127.0.0.1 -p 15353 +time=2 +tries=1 +tcp +ignore order-demo.manual-order.example A
```

**Expect:** answer from **default** backend `127.0.0.1:15300` (not vip `15301`). The script would select pool `vip`, but the following built-in `set_pool` overrides it — proving order matters.

To see **vip** routing instead, swap the two actions in the YAML (or remove the trailing `set_pool`).

---

## 6. `conduitctl validate` — Rhai compile failure

With Conduit stopped:

```bash
cargo run -p conduitctl -- validate --file tests/fixtures/config/with-rhai-syntax-error.yaml
echo exit=$?
```

**Expect:** non-zero exit; error mentions script compile (invalid Rhai syntax). YAML-only validation passes for structurally valid files; validate also builds the runtime snapshot including script compile.

Compare with a good file:

```bash
cargo run -p conduitctl -- validate --file tests/manual/config/09-ordered-actions.yaml
echo exit=$?
```

**Expect:** exit 0.
