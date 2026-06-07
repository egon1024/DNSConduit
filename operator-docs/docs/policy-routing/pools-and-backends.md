# Pools and backends

This page explains how Conduit groups upstream DNS servers into [pools](/glossary/index.md#pool), selects a [backend](/glossary/index.md#backend) within a [pool](/glossary/index.md#pool), and forwards queries to it.

## Overview

[Pools](/glossary/index.md#pool) and [backends](/glossary/index.md#backend) are the organizational pattern Conduit uses to route forwarded DNS queries.

A [backend](/glossary/index.md#backend) is a configured upstream destination Conduit forwards DNS queries to. Each backend carries settings that control how Conduit reaches and uses that destination (for example, address, port, and load-balancing weight in current releases).

A [pool](/glossary/index.md#pool) is a named group of [backends](/glossary/index.md#backend). [Rules](/policy-routing/rules-and-actions.md) and scripts select a pool by name; Conduit then picks one backend inside that pool (see [Backend weights](#backend-weights)) and forwards the query.

On the [dataplane](/glossary/index.md#dataplane), Conduit selects the [pool](/glossary/index.md#pool) and [backend](/glossary/index.md#backend) during the **Route** phase of each [transaction](/glossary/index.md#transaction), after [request rules](/policy-routing/rules-and-actions.md) run. For the full query pipeline, see [Architecture and packet path](/concepts/architecture-and-packet-path.md). If nothing sets a pool, Conduit uses the pool named `default`, or the first pool in configuration. If the selected pool is missing or has no backends, Conduit responds with **SERVFAIL**. A [retry](/glossary/index.md#retry) may target a different pool ([Retries and transactions](/policy-routing/retries-and-transactions.md)).

## Configuration

[Pools](/glossary/index.md#pool) are declared under the top-level `pools:` key in the [config file](/control-plane/config-file.md). Each pool has a unique `name` and an unordered list of `backends`. Duplicate pool names or a pool with an empty `backends` list are rejected when the config is loaded or validated. A runnable installation needs at least one pool with at least one backend.

Minimal example (single pool, single backend):

```yaml
pools:
  - name: default
    backends:
      - address: "127.0.0.1:5300"
        weight: 100
```

| Field | Meaning |
|-------|---------|
| `name` | Pool identifier used by [rules](/policy-routing/rules-and-actions.md) (`set_pool`, `retry_pool`), [Rhai](/rhai/index.md), and as the `pool` label on [metrics](/observability/metrics.md). |
| `backends` | One or more upstream destinations in this pool. |
| `address` | Upstream resolver as `ip:port` (IPv6 addresses use bracket notation, for example `[2001:db8::1]:53`). |
| `weight` | Optional load-balancing weight; see [Backend weights](#backend-weights). |
| `sources_v4` | Optional list of local IPv4 addresses for upstream egress to this pool’s backends; see [Dual-stack forwarding](/guides/dual-stack-forwarding.md). |
| `sources_v6` | Optional list of local IPv6 addresses for upstream egress to this pool’s backends. |

For every pool and backend field, defaults, and validation rules, see [Reference: pools](/reference/config-schema/pools.md).

Other top-level blocks (`listeners`, `forward`, `rules`, and so on) are required for a runnable config but are documented on their own pages. A complete minimal file appears in [Minimal configuration](/getting-started/minimal-configuration.md).

## Backend weights

When a [pool](/glossary/index.md#pool) contains more than one [backend](/glossary/index.md#backend), Conduit distributes queries among them using each backend’s **weight**. Weights are positive integers; **`weight` is optional** — if omitted, the effective weight is **100**.

Example — two backends with a 70/30 split:

```yaml
pools:
  - name: default
    backends:
      - address: "10.0.0.1:53"
        weight: 70
      - address: "10.0.0.2:53"
        weight: 30
```

Over many queries, traffic approximates the configured weight ratio.

Pool weights can be changed at runtime through the [control plane](/glossary/index.md#control-plane) (for example via `ApplyConfig`); see [Control plane workflows](/guides/control-plane-workflows.md).

## Multiple pools

Define more than one [pool](/glossary/index.md#pool) when different queries should use different upstream groups — for example public recursive resolvers for the internet and a separate resolver for internal zones (split horizon).

Only queries that match a [rule](/policy-routing/rules-and-actions.md) need an explicit `set_pool`; everything else uses the pool named `default`, or the first pool in the file if there is no `default` pool (see [Overview](#overview)).

```yaml
pools:
  - name: default
    backends:
      - address: "100.100.100.100:53"
        weight: 100
  - name: internal
    backends:
      - address: "10.0.1.53:53"
        weight: 100

rules:
  match_mode: first_match   # only supported mode today
  rules:
    - name: internal-zones
      hook: request
      selectors:
        - type: qname_suffix
          value: ".corp.example."
      actions:
        - type: set_pool
          value: internal
```

Queries for names ending in `.corp.example.` use the **internal** pool; all other queries use **default** without an extra catch-all rule.

The pool name in `set_pool`, `retry_pool` (or in Rhai’s `set_pool(...)` / `set_retry_pool(...)`) must match a `name` under `pools:`. Additional [selectors](/glossary/index.md#selector) — query name, query type, response code, tags, and others — are covered on [Rules and actions](/policy-routing/rules-and-actions.md). [Retries](/policy-routing/retries-and-transactions.md) can send a later attempt to a different pool when an upstream fails.

## Related topics

- [Glossary](/glossary/index.md) — [pool](/glossary/index.md#pool), [backend](/glossary/index.md#backend), [transaction](/glossary/index.md#transaction), [retry](/glossary/index.md#retry)
- [Rules and actions](/policy-routing/rules-and-actions.md) — how request and response rules select pools
- [Retries and transactions](/policy-routing/retries-and-transactions.md) — failing over to another pool or backend
- [Minimal configuration](/getting-started/minimal-configuration.md) — smallest runnable config including `pools:`
- [Architecture and packet path](/concepts/architecture-and-packet-path.md) — pipeline phases and the Route step
- [Reference: pools](/reference/config-schema/pools.md) — config schema (field reference)
