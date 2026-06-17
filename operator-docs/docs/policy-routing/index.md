# Policy & routing

How Conduit decides **where** each client query goes upstream, **what** to change on the way out, and **when** to try again. This section covers declarative `rules:`, upstream `pools:` and [backends](/glossary/index.md#backend), and [retry](/glossary/index.md#retry) limits on the [dataplane](/glossary/index.md#dataplane).

Read [Architecture and packet path](/concepts/architecture-and-packet-path.md) first for pipeline phases and hook placement; YAML field lists live on the topic pages below and in [Reference](/reference/config-schema/rules.md).

## How the pieces fit

| Concern | Config | Topic page |
|---------|--------|------------|
| Match queries; set pool, tags, egress, or drop | `rules:` | [Rules and actions](/policy-routing/rules-and-actions.md) |
| Scripted policy on matching rules | `rules:` + `type: rhai` | [Rhai](/rhai/index.md) (Rule Rhai) |
| Group upstream resolvers and load-balance | `pools:` | [Pools and backends](/policy-routing/pools-and-backends.md) |
| Caps and behavior on repeat attempts | `orchestrator:` | [Retries and transactions](/policy-routing/retries-and-transactions.md) |
| Default upstream timeout and egress address lists | `forward:` | [Dual-stack forwarding](/guides/dual-stack-forwarding.md) |

On each [transaction](/glossary/index.md#transaction), policy currently runs at two hooks on the query path:

```mermaid
flowchart TD
  Parse[Parse] --> Req[Request rules]
  Req --> Route[Route — pool + backend]
  Route --> Fwd[Forward]
  Fwd --> Wait[Wait for response]
  Wait --> Res[Response rules]
  Res -->|continue / no retry| Send[Send]
  Res -->|retry| Route
  Req -->|drop| Drop[Drop]
  Res -->|drop| Drop
```

1. **[Request rules](/concepts/architecture-and-packet-path.md#request-rules)** run **once** after [Parse](/concepts/architecture-and-packet-path.md#parse). [Selectors](/glossary/index.md#selector) test the query; [actions](/glossary/index.md#action) can set [pool](/glossary/index.md#pool), [tags](/glossary/index.md#tags), upstream egress source, or **drop**. No match → default [pool](/glossary/index.md#pool) path at [Route](/concepts/architecture-and-packet-path.md#route).
2. **[Route](/concepts/architecture-and-packet-path.md#route)** picks a [backend](/glossary/index.md#backend) in the selected pool (sticky weighted choice on the first attempt).
3. After an upstream answer or forward timeout, **[Response rules](/concepts/architecture-and-packet-path.md#response-rules)** continue to [Send](/concepts/architecture-and-packet-path.md#send), **drop**, or request a **retry** (`retry` / `retry_now`) — looping back to **Route**, not re-running request rules.
4. **[Send](/concepts/architecture-and-packet-path.md#send)** returns the final answer (or the transaction ends with drop or synthesized **SERVFAIL** when limits or pool exhaustion apply).

[Rules and actions](/policy-routing/rules-and-actions.md) use **`match_mode: first_match`**: on each hook, Conduit walks the rule list top to bottom and stops at the first rule whose selectors all match. Actions on that rule — built-in and optional [Rule Rhai](/glossary/index.md#rule-rhai) — run in **list order**. See [Rhai](/rhai/index.md).

## Read in order

1. [Rules and actions](/policy-routing/rules-and-actions.md) — `rules:` hooks, selectors, built-in actions, reload behavior
2. [Rhai](/rhai/index.md) — Rule Rhai when built-in actions are not enough
3. [Pools and backends](/policy-routing/pools-and-backends.md) — `pools:` layout, weights, default pool, **SERVFAIL** when routing fails
4. [Retries and transactions](/policy-routing/retries-and-transactions.md) — response-hook retries, backend exclusion per attempt, `orchestrator` caps

## Prerequisites

- [Minimal configuration](/getting-started/minimal-configuration.md) — smallest runnable file (`listeners`, `pools`)
- [Config file](/control-plane/config-file.md) — where `rules:`, `pools:`, `orchestrator:`, and `forward:` sit in the top-level map

## Config reference

| Block | Reference |
|-------|-----------|
| `rules:` | [Reference: rules](/reference/config-schema/rules.md) |
| `pools:` | [Reference: pools](/reference/config-schema/pools.md) |
| `orchestrator:` | [Global limits](/policy-routing/retries-and-transactions.md#global-limits-orchestrator); overview in [Config file](/control-plane/config-file.md) |
| `forward:` | [Dual-stack forwarding](/guides/dual-stack-forwarding.md) |

Rule and pool changes load into the [runtime snapshot](/glossary/index.md#runtime-snapshot) on reload or apply for **new** queries; in-flight [transactions](/glossary/index.md#transaction) keep the policy they started with. See [Configuration model](/control-plane/configuration-model.md) and [When changes to rules take effect](/policy-routing/rules-and-actions.md#when-changes-to-rules-take-effect).

## Related

- [Dual-stack forwarding](/guides/dual-stack-forwarding.md) — `forward.sources_*`, pool `sources_*`, per-query `set_source_v4` / `set_source_v6`
- [Rhai](/rhai/index.md) — Rule Rhai on request and response hooks
- [Built-in metrics](/observability/built-in-metrics.md) — `pool` labels and forward health after you add pools and rules
