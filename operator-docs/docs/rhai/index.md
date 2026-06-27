# Rhai

Conduit embeds the [Rhai](https://rhai.rs/) scripting language for **rule policy** — [Rule Rhai](/glossary/index.md#rule-rhai), referenced from `rules:`. Scripts run on the request and response hooks, share **host APIs** (lookups, custom metrics), and act on a sandboxed **`txn`** policy object.

This section is the **language-first reference** for writing Rhai in Conduit. Feature behavior and YAML wiring live in [Policy & routing](/policy-routing/index.md).

## What Rhai for rules does

| Goal | Config | Script API | Status |
|------|--------|------------|--------|
| Policy on a matching rule — pool, tags, drop, retry, egress, sampling, metrics | `rules:` → `type: rhai` | **`txn`** ([Transaction API](/rhai/transaction-api.md)) | **Shipped** |

Rhai for rules does **not** edit DNS wire bytes (qname rewrite, response mutation, EDNS/EDE, RD-bit control). It also does **not** replace [rules](/policy-routing/rules-and-actions.md): [request](/concepts/architecture-and-packet-path.md#request-rules) and [response](/concepts/architecture-and-packet-path.md#response-rules) rules run on their hooks, and `type: rhai` is one action among the declarative ones.

## Read in order

1. [Rule Rhai overview](/rhai/rule-rhai.md) — when to use scripts, how they attach to rules, minimal example
2. [Rhai policy](/guides/rhai-policy.md) — blocklist drop and CSV pool routing labs (request hook)
3. [Hooks and phases](/rhai/hooks-and-phases.md) — request vs response hooks; slow-login and tag + dnstap pairing
4. [Transaction API](/rhai/transaction-api.md) — `txn` methods and YAML equivalents

Behavioral context: [Rules and actions](/policy-routing/rules-and-actions.md), [Policy & routing](/policy-routing/index.md).

### Shared host APIs

- [Data sources and lookups](/rhai/data-sources-and-lookups.md) — `data_sources:` and `table_lookup`
- [User metrics](/rhai/user-metrics.md) — `metric_inc` → `conduit_user_*`
- [Sandbox limits](/rhai/sandbox-limits.md) — `rhai:` limits for Rhai for rules

## Prerequisites

- [Architecture and packet path](/concepts/architecture-and-packet-path.md) — pipeline phases and where rules run today
- [Config file](/control-plane/config-file.md) — path resolution for `.rhai` and CSV paths
- [Glossary](/glossary/index.md) — [Rule Rhai](/glossary/index.md#rule-rhai)

## Related

- [Policy & routing](/policy-routing/index.md) — declarative policy and Rhai for rules today
