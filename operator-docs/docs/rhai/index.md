# Rhai

Conduit embeds the [Rhai](https://rhai.rs/) scripting language for **rule policy** — [Rule Rhai](/glossary/index.md#rule-rhai), referenced from `rules:`. Scripts run on the request and response hooks against five host scope objects: **`txn`**, **`runtime`**, **`lookup`**, **`metrics`**, and **`log`**.

This section is the **language-first reference** for writing Rhai in Conduit. Feature behavior and YAML wiring are described in [Policy & routing](/policy-routing/index.md).

## Host API architecture

Every hook invocation exposes five scopes. Each has a distinct mutability class:

| Scope | Role | Reference |
|-------|------|-----------|
| **`txn`** | Per-query policy — pools, tags, drop/retry, egress, question/response reads | [Transaction API (`txn`)](/rhai/txn-api.md) |
| **`runtime`** | Read-only process state — **`runtime.routing()`** for health-aware routing | [Runtime API](/rhai/runtime-api.md) |
| **`lookup`** | Read-only CSV tables from **`data_sources:`** via **`lookup()`** | [Data sources and lookups](/rhai/data-sources-and-lookups.md) |
| **`metrics`** | Write-only user counters → `conduit_user_*` | [User metrics](/rhai/user-metrics.md) |
| **`log`** | Script log lines via Conduit tracing | [Script logging](/rhai/script-logging.md) |

Start with [Host API overview](/rhai/host-api.md) for the mental model and consistency rules.

## What Rhai for rules does

| Goal | Config | Primary API | Status |
|------|--------|-------------|--------|
| Policy on a matching rule — pool, tags, drop, retry, egress, sampling | `rules:` → `type: rhai` | **`txn`** + shared scopes above | **Shipped** |
| Health-aware pool/backend branching in scripts | `pools[].health` + Rhai | **`runtime.routing()`** | **Shipped** |

Rhai for rules does **not** edit DNS wire bytes (qname rewrite, response mutation, EDNS/EDE, RD-bit control). It also does **not** replace [rules](/policy-routing/rules-and-actions.md): [request](/concepts/architecture-and-packet-path.md#request-rules) and [response](/concepts/architecture-and-packet-path.md#response-rules) rules run on their hooks, and `type: rhai` is one action among the declarative ones.

## Read in order

1. [Rule Rhai overview](/rhai/rule-rhai.md) — when to use scripts, how they attach to rules, minimal example
2. [Host API overview](/rhai/host-api.md) — five scope objects
3. [Rhai policy](/guides/rhai-policy.md) — blocklist drop and CSV pool routing labs (request hook)
4. [Hooks and phases](/rhai/hooks-and-phases.md) — request vs response hooks; slow-login and tag + dnstap pairing
5. [Transaction API (`txn`)](/rhai/txn-api.md) — per-query policy methods and YAML equivalents
6. [Runtime API](/rhai/runtime-api.md) — **`runtime.routing()`** pool/backend health reads for policy branching

Behavioral context: [Rules and actions](/policy-routing/rules-and-actions.md), [Backend health](/policy-routing/backend-health.md), [Policy & routing](/policy-routing/index.md).

## Prerequisites

- [Architecture and packet path](/concepts/architecture-and-packet-path.md) — pipeline phases and where rules run today
- [Config file](/control-plane/config-file.md) — path resolution for `.rhai` and CSV paths
- [Glossary](/glossary/index.md) — [Rule Rhai](/glossary/index.md#rule-rhai)

## Related

- [Policy & routing](/policy-routing/index.md) — declarative policy and Rhai for rules today
