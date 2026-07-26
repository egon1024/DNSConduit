# Config schema: shutdown

This page lists the fields for the top-level **`shutdown:`** block — how Conduit drains in-flight [transactions](/glossary/index.md#transaction) when the process is asked to stop. For the behavior in context, see [Runtime and concurrency — Graceful drain on shutdown](/concepts/runtime-and-concurrency.md#graceful-drain-on-shutdown).

## `shutdown`

| Property | Value |
|----------|--------|
| **Type** | Mapping (object) |
| **Required** | No — defaults apply when the block is omitted |
| **Location** | Top-level key in the [config file](/control-plane/config-file.md) |

When **`shutdown:`** is omitted, Conduit drains in-flight transactions for up to **5 s** on shutdown — the defaults in the table below.

## Block fields

| Field {: .column-no-wrap } | Type | Required | Default | Description |
|-------|------|----------|---------|-------------|
| `drain` | boolean | no | **true** | Whether to wait for in-flight [transactions](/glossary/index.md#transaction) to finish before tearing down listeners. `false` skips the wait and stops immediately. |
| `drain_timeout_ms` | integer | no | **5000** | Upper bound (milliseconds) on the drain wait. When it elapses, Conduit logs how many transactions remain and stops anyway. `0` checks once and proceeds without waiting. |

## How the drain works

On **SIGTERM** or **SIGINT** (Ctrl+C) — **not** SIGHUP, which [reloads from disk](/glossary/index.md#reload-from-disk) — Conduit stops the [control plane](/glossary/index.md#control-plane) and metrics endpoints, then waits for every active [transaction](/glossary/index.md#transaction) slot to finish before closing listeners. This includes [`split_io`](/concepts/runtime-and-concurrency.md#split-io-runtime) transactions parked at [Wait for response](/concepts/architecture-and-packet-path.md#wait-for-response). A clean drain is logged at debug; a timeout logs the remaining count. Drain applies to all [dataplane runtime models](/concepts/runtime-and-concurrency.md#runtime-models).

### Second signal exits immediately

While the drain is in progress, a **second** SIGTERM/SIGINT abandons the remaining wait and proceeds straight to listener teardown. Use it to force an immediate exit instead of waiting out `drain_timeout_ms`.

### Relationship to other timeouts

- **`orchestrator.max_txn_duration_ms`** caps the lifetime of a *single* query; **`shutdown.drain_timeout_ms`** caps the *total* shutdown wait across all in-flight queries. They are independent — size the drain timeout from how long you are willing to delay process exit, not from per-query limits. See [Orchestrator](/reference/config-schema/orchestrator.md).
- **`forward.timeout_ms`** bounds each upstream wait. A transaction stuck on a slow upstream finishes (or times out) within that bound, so a drain can take up to `forward.timeout_ms` to settle even under light traffic. See [Forward](/reference/config-schema/forward.md).

## Reload and overlay

Drain settings are **dynamic** — no restart required. Conduit reads `shutdown.drain` and `shutdown.drain_timeout_ms` from the live [runtime snapshot](/glossary/index.md#runtime-snapshot) at the moment shutdown begins, so the most recently applied or reloaded values are the ones that take effect.

| Change | Effect |
|--------|--------|
| Edit **`shutdown:`** on disk + [reload](/glossary/index.md#reload-from-disk) (SIGHUP or **`conduitctl reload`**), or **`conduitctl apply`** a patch including **`shutdown:`** | Takes effect immediately; the next shutdown uses the new drain settings |
| **Restart** | Not required for **`shutdown:`** changes |

Because the value is read when shutdown starts, a drain already in progress keeps the timeout it began with; a change applied mid-drain affects the *next* shutdown. Unlike start-time settings such as the **`dataplane.runtime`** model, the `shutdown:` block does not require a restart — see [Architecture — Runtime snapshot](/concepts/architecture-and-packet-path.md#runtime-snapshot).

## Validation summary

**`shutdown:`** has no rejecting validation rules: `drain: false` disables the wait and `drain_timeout_ms: 0` performs a single check without waiting. Validate config with `conduitctl validate --file …`.

## Example configuration

Wait up to 2 seconds for in-flight queries before exiting:

```yaml
shutdown:
  drain: true
  drain_timeout_ms: 2000
```

Disable draining (close listeners as soon as the process is signalled):

```yaml
shutdown:
  drain: false
```

## Related topics

- [Runtime and concurrency — Graceful drain on shutdown](/concepts/runtime-and-concurrency.md#graceful-drain-on-shutdown)
- [Orchestrator](/reference/config-schema/orchestrator.md) — `max_txn_duration_ms` (per-query lifetime)
- [Forward](/reference/config-schema/forward.md) — `forward.timeout_ms` (per-upstream wait)
- [Configuration model](/control-plane/configuration-model.md) — snapshots, reload, and what needs a restart
- [Config schema overview](/reference/config-schema/index.md)
