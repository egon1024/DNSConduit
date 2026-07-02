---
toc_depth: 3
toc_collapsible: true
---

# Script logging (`log`)

The **`log`** scope object emits structured lines through Conduit’s **tracing** pipeline. This is **not** file I/O from Rhai — messages appear in process logs with script correlation fields.

See [Host API overview](/rhai/host-api.md#five-scope-objects) for how **`log`** fits alongside **`txn`**, **`runtime`**, **`lookup`**, and **`metrics`**.

## Methods

<p class="txn-api-index" markdown="1">

**Methods:** [`log.info(msg)`](#loginfomsg) · [`log.warn(msg)`](#logwarnmsg)

</p>

<div class="txn-api-entry" markdown="1">

### `log.info(msg)` {#loginfomsg}

<div class="txn-api-brief" markdown="1">

Request + response hook · `msg`: string · no return

Info-level script log line (rate-limited).

</div>

<div class="txn-api-reference-panel" markdown="1" hidden>

#### Behavior

- Writes at **info** severity with **`script`**, **`rule`**, and **`txn_id`** fields (same pipeline as other Conduit tracing).
- **Rate limit:** first call per script/rule per [snapshot generation](/control-plane/configuration-model.md), then every **100** calls.
- Messages longer than **512** characters are truncated.
- Use for debug/canary branches — not per-query logging at high QPS.

</div>

</div>

<div class="txn-api-entry" markdown="1">

### `log.warn(msg)` {#logwarnmsg}

<div class="txn-api-brief" markdown="1">

Request + response hook · `msg`: string · no return

Warn-level script log line (same rate limit as `log.info`).

</div>

</div>

## Example

```rhai
if txn.has_tag("debug") {
    log.info(`policy matched txn=${txn.txn_id()} pool=${txn.selected_pool()}`);
}
```

Pair with **`txn.txn_id()`** for correlation — see [Transaction API — Introspection](/rhai/txn-api.md#introspection).

## Related

- [Logging](/observability/logging.md) — operator log configuration
- [Sandbox limits](/rhai/sandbox-limits.md) — script logging rate limits
- [Built-in metrics — script errors](/observability/built-in-metrics.md#conduit_script_errors_total) — eval failures (separate from `log`)
