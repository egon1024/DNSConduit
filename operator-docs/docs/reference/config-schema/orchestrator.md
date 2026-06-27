# Config schema: orchestrator

Field reference for the top-level **`orchestrator:`** block — global limits on [retries](/glossary/index.md#retry), [transaction](/glossary/index.md#transaction) duration, and in-flight transaction capacity. For when retries fire, pool selection on re-entry, and client-visible outcomes, see [Retries and transactions](/policy-routing/retries-and-transactions.md).

## `orchestrator`

| Property | Value |
|----------|--------|
| **Type** | Mapping (object) |
| **Required** | No — defaults apply when the block is omitted |
| **Location** | Top-level key in the [config file](/control-plane/config-file.md) |

When **`orchestrator:`** is omitted, Conduit applies the defaults in the table below at parse time.

## Block fields

| Field {: .column-no-wrap } | Type | Required | Default | Description |
|-------|------|----------|---------|-------------|
| `max_attempts` | integer | no | **3** | Maximum [Route](/concepts/architecture-and-packet-path.md#route) → [Forward](/concepts/architecture-and-packet-path.md#forward) cycles for one client query (includes the first attempt). Must be **≥ 1** when set explicitly. |
| `max_txn_duration_ms` | integer | no | **5000** | Wall-clock limit for the whole transaction from start through [Send](/concepts/architecture-and-packet-path.md#send) or [drop](/concepts/architecture-and-packet-path.md#parse). Checked before each [Route](/concepts/architecture-and-packet-path.md#route). |
| `txn_table_capacity` | integer | no | **1024** | Capacity of the in-flight [transaction](/glossary/index.md#transaction) table on the [datapath](/glossary/index.md#dataplane) — bounds concurrent queries being processed, not per-query retry count. |

### How limits interact

Conduit checks **`max_attempts`** and **`max_txn_duration_ms`** before each [Route](/concepts/architecture-and-packet-path.md#route). A [retry](/glossary/index.md#retry) from [Response rules](/concepts/architecture-and-packet-path.md#response-rules) re-enters at Route only while both limits allow and the target [pool](/glossary/index.md#pool) still has an unused [backend](/glossary/index.md#backend).

When a limit is exceeded (or the pool is exhausted), Conduit typically sets **SERVFAIL** and moves to [Send](/concepts/architecture-and-packet-path.md#send) instead of forwarding again — see [Retries and transactions — What the client sees when limits hit](/policy-routing/retries-and-transactions.md#what-the-client-sees-when-limits-hit).

**`txn_table_capacity`** is independent of **`max_attempts`**: it caps how many transactions the datapath tracks at once under load. See [Runtime and concurrency — Worker counts and limits](/concepts/runtime-and-concurrency.md#worker-counts-and-limits).

## Reload and overlay

| Change | Effect |
|--------|--------|
| Edit **`orchestrator:`** on disk + reload | New limits apply to **later** queries — no process restart required |
| **`conduitctl apply`** patch including **`orchestrator:`** | Replaces the file-layer **`orchestrator:`** section in the overlay when present; hot for new queries after successful apply |

In-flight [transactions](/glossary/index.md#transaction) keep the limits they started under.

## Validation summary

| Rule | Error if violated |
|------|-------------------|
| `max_attempts` ≥ **1** when `orchestrator:` present | `orchestrator.max_attempts must be >= 1` |

Validate with `conduitctl validate --file …`.

## Example configuration

```yaml
orchestrator:
  max_attempts: 5
  max_txn_duration_ms: 8000
  txn_table_capacity: 2048
```

Declarative retry example using defaults: [Retries and transactions — Declarative examples](/policy-routing/retries-and-transactions.md#declarative-examples).

## Related topics

- [Retries and transactions](/policy-routing/retries-and-transactions.md) — `retry`, `set_retry_pool`, pool exhaustion
- [Rules and actions](/policy-routing/rules-and-actions.md) — response-hook retry actions
- [Architecture and packet path — Retries and re-entry](/concepts/architecture-and-packet-path.md#retries-and-re-entry)
- [Built-in metrics](/observability/built-in-metrics.md) — [`conduit_retries_total`](/observability/built-in-metrics.md#conduit_retries_total)
- [Minimal configuration — Defaults](/getting-started/minimal-configuration.md#defaults-you-do-not-need-to-write-yet)
- [Config schema overview](/reference/config-schema/index.md)
