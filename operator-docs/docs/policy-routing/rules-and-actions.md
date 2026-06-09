# Rules and actions

This page explains how Conduit applies **declarative policy** on the [dataplane](/glossary/index.md#dataplane) at [Request rules](/concepts/architecture-and-packet-path.md#request-rules) and [Response rules](/concepts/architecture-and-packet-path.md#response-rules). For scripting, [WASM](/glossary/index.md#wasm), and [sidecar](/glossary/index.md#sidecar) models, see [Extensibility](/concepts/extensibility.md). For pool and backend selection after policy runs, see [Pools and backends](/policy-routing/pools-and-backends.md).

## Overview

Rules live under `rules:` in your [config file](/control-plane/config-file.md). Each **rule** has:

- A **hook** — `request` or `response` (maps to [Request rules](/concepts/architecture-and-packet-path.md#request-rules) or [Response rules](/concepts/architecture-and-packet-path.md#response-rules))
- **[Selectors](/glossary/index.md#selector)** — conditions on the [transaction](/glossary/index.md#transaction) (query name, type, response code, [tags](/glossary/index.md#tags), …)
- **[Actions](/glossary/index.md#action)** — built-in effects when the rule matches

Conduit evaluates rules in **`first_match`** order on each hook: the first rule whose selectors all match wins; later rules are skipped for that query.

When a matching rule includes a **`rhai`** [action](/glossary/index.md#action), Conduit runs the linked [Rhai](/rhai/index.md) script **after** applying built-in actions on that rule. Script API: [Rhai](/rhai/index.md).

```yaml
rules:
  match_mode: first_match
  rules:
    - name: internal-a
      hook: request
      selectors:
        - type: qtype
          value: A
      actions:
        - type: set_pool
          value: internal
        - type: set_source_v4
          value: "10.0.0.5"
```

Reload and validation: rules compile into the [runtime snapshot](/glossary/index.md#runtime-snapshot) on **SIGHUP**, `conduitctl reload`, or `conduitctl apply`. See [Configuration model](/control-plane/configuration-model.md).

## Action order on one rule

**Actions on a single rule run in list order** (top to bottom). Order matters when multiple actions touch the same [transaction](/glossary/index.md#transaction) fields.

When you use **`set_pool`** and **`set_source_v4`** / **`set_source_v6`** on the **same** rule, list **`set_pool` first**, then the source action — so [Forward](/concepts/architecture-and-packet-path.md#forward) checks the override against the allowed addresses for the pool you just selected.

The selected [pool](/glossary/index.md#pool) can also come from an **earlier** matching rule or from the default pool when no rule sets one. At [Forward](/concepts/architecture-and-packet-path.md#forward), Conduit still requires the override to be in the **allowed set for that pool** (global `forward.sources_*` ∪ that pool’s `sources_*`). If the override is not allowed, Conduit **falls back to round-robin** among configured sources — same behavior as [Rhai](/rhai/index.md) `set_source_v4` / `set_source_v6`.

## Request-hook actions

Use on `hook: request` (the [Request rules](/concepts/architecture-and-packet-path.md#request-rules) phase, before [Route](/concepts/architecture-and-packet-path.md#route)).

| Action | `value` | Effect |
|--------|---------|--------|
| `set_pool` | Pool name | Sets the target [pool](/glossary/index.md#pool) for [Route](/concepts/architecture-and-packet-path.md#route) |
| `set_tag` | `key=value` or `key` (→ `true`) | Sets a [tag](/glossary/index.md#tags) on the [transaction](/glossary/index.md#transaction) |
| `set_source_v4` | IPv4 address | Pins upstream egress to this local IPv4 address for this query |
| `set_source_v6` | IPv6 address | Pins upstream egress to this local IPv6 address for this query |
| `drop` | (ignored) | Ends the query with **no** DNS reply |
| `rhai` | Script path | Runs a [Rhai](/rhai/index.md) request script after built-in actions |

**`set_source_v4` / `set_source_v6`**

- **Request hook only** — not valid on `hook: response`.
- The address must appear in **`forward.sources_v4`** / **`forward.sources_v6`** or a pool’s **`sources_v4`** / **`sources_v6`** (union checked at config validation).
- For when to use static pool sources vs rule actions vs Rhai, see [Dual-stack forwarding](/guides/dual-stack-forwarding.md#choosing-an-egress-source).

## Response-hook actions

Use on `hook: response` (the [Response rules](/concepts/architecture-and-packet-path.md#response-rules) phase, after upstream answer or forward timeout).

| Action | `value` | Effect |
|--------|---------|--------|
| `retry_pool` | Pool name | Sets the pool for a [retry](/glossary/index.md#retry) ([Route](/concepts/architecture-and-packet-path.md#route) → [Forward](/concepts/architecture-and-packet-path.md#forward)) when combined with retry intent |
| `set_rcode` | RCODE name | Sets response code metadata (for example before [Send](/concepts/architecture-and-packet-path.md#send)) |
| `drop` | (ignored) | Ends the query with **no** DNS reply |
| `rhai` | Script path | Runs a [Rhai](/rhai/index.md) response script after built-in actions |

**`set_source_v4` / `set_source_v6` are not supported on the response hook** (same as [Rhai](/rhai/index.md) — egress is chosen before [Forward](/concepts/architecture-and-packet-path.md#forward)).

Retry semantics and orchestrator limits: [Retries and transactions](/policy-routing/retries-and-transactions.md).

## Selectors

Each selector on a rule must match (logical **AND**). Supported selector types in current releases:

| Type | Tests |
|------|--------|
| `qname_suffix` | Query name suffix |
| `qname_exact` | Exact query name |
| `qtype` | Query type (for example `A`, `AAAA`) |
| `rcode` | Response code (response hook) |
| `tag` | [Tag](/glossary/index.md#tags) presence or value |

An empty `selectors:` list matches every query on that hook.

## Validation errors (common)

| Message (typical) | Cause |
|-------------------|--------|
| `set_source_v4 … only valid on request hook` | Source action on `hook: response` |
| `set_source_v4 … not in configured sources_v4` | Address not in global or pool `sources_v4` union |
| `set_source_v4 requires forward.sources_v4 or pool sources_v4` | No v4 sources configured anywhere |
| `unknown action type` | Typo in `type:` |

Field reference: [Config schema: rules](/reference/config-schema/rules.md).

## Related topics

- [Architecture and packet path](/concepts/architecture-and-packet-path.md) — where [Request rules](/concepts/architecture-and-packet-path.md#request-rules) and [Response rules](/concepts/architecture-and-packet-path.md#response-rules) run
- [Extensibility](/concepts/extensibility.md) — built-in rules vs [Rhai](/glossary/index.md#rhai) / future plugins
- [Pools and backends](/policy-routing/pools-and-backends.md) — `set_pool`, default pool, weights
- [Dual-stack forwarding](/guides/dual-stack-forwarding.md) — egress source addresses
- [Rhai](/rhai/index.md) — scripts when built-in actions are not enough
