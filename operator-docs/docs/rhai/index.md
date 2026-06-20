# Rhai

Conduit embeds the [Rhai](https://rhai.rs/) scripting language in two **separate** datapath runtimes. Both use `.rhai` files and share some **host APIs** (lookups, custom metrics), but they attach to different config, run at different pipeline points, and expose **different script objects**.

This section is the **language-first reference** for writing Rhai in Conduit. Feature behavior and YAML wiring live in [Policy & routing](/policy-routing/index.md) (rules) and [Processor chains](/processor-chains/index.md) (`processors:` — planned).

## Which flavor do I need?

| Goal | Flavor | Config | Script API | Status |
|------|--------|--------|------------|--------|
| Policy on a matching rule — pool, tags, drop, retry, egress, sampling, metrics | **Rhai for rules** ([Rule Rhai](/glossary/index.md#rule-rhai)) | `rules:` → `type: rhai` | **`txn`** ([Transaction API](/rhai/transaction-api.md)) | **Shipped** |
| DNS wire editing — qname rewrite, response mutation, EDNS/EDE, RD-bit control | **Rhai for processor chains** ([Processor-chain Rhai](/glossary/index.md#processor-chain-rhai)) | `processors:` chain links | **Message API** ([`conduit-dns`](/rhai/message-api.md)) | **Planned** |

Rhai for rules does **not** edit DNS wire bytes. Rhai for processor chains does **not** replace [rules](/policy-routing/rules-and-actions.md) — [request](/concepts/architecture-and-packet-path.md#request-rules) and [response](/concepts/architecture-and-packet-path.md#response-rules) rules still run on their hooks first; processors attach at fixed pipeline slots afterward. See [Processor chains](/processor-chains/index.md).

## Read in order

### Rhai for rules (shipped)

1. [Rule Rhai overview](/rhai/rule-rhai.md) — when to use scripts, how they attach to rules, minimal example
2. [Hooks and phases](/rhai/hooks-and-phases.md) — request vs response hooks, phase guards
3. [Transaction API](/rhai/transaction-api.md) — `txn` methods and YAML equivalents

Behavioral context: [Rules and actions](/policy-routing/rules-and-actions.md), [Policy & routing](/policy-routing/index.md).

### Rhai for processor chains (planned)

1. [Processor chains](/processor-chains/index.md) — `processors:` config, pipeline attachment, backends
2. [Processor-chain Rhai overview](/rhai/processor-chain-rhai.md) — how Rhai links fit in a chain
3. [Message API](/rhai/message-api.md) — wire-editing surface (`conduit-dns`)

### Shared host APIs (Rhai and planned backends)

- [Data sources and lookups](/rhai/data-sources-and-lookups.md) — `data_sources:` and `table_lookup`
- [User metrics](/rhai/user-metrics.md) — `metric_inc` → `conduit_user_*`
- [Sandbox limits](/rhai/sandbox-limits.md) — `rhai:` limits for Rhai for rules (processor-chain limits documented when shipped)

## Prerequisites

- [Architecture and packet path](/concepts/architecture-and-packet-path.md) — pipeline phases and where rules run today
- [Config file](/control-plane/config-file.md) — path resolution for `.rhai` and CSV paths
- [Glossary](/glossary/index.md) — [Rule Rhai](/glossary/index.md#rule-rhai), [Processor-chain Rhai](/glossary/index.md#processor-chain-rhai), [Processor chains](/glossary/index.md#processor-chains)

## Related

- [Planned plugin models](/concepts/planned-plugin-models.md) — WASM, sidecar, and processor-chain roadmap
- [Policy & routing](/policy-routing/index.md) — declarative policy and Rhai for rules today
