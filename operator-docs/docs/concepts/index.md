# Concepts

Mental models for how Conduit handles DNS — the query path on the [dataplane](/glossary/index.md#dataplane) and where policy plugs in. Read these before diving into config field lists and runbooks; YAML syntax and operational detail live in other sections.

**Read in order:**

1. [Architecture and packet path](/concepts/architecture-and-packet-path.md) — listeners, [pipeline phases](/concepts/architecture-and-packet-path.md#pipeline-phases), [transactions](/glossary/index.md#transaction), [runtime snapshot](/glossary/index.md#runtime-snapshot), and how a query reaches upstream and back

**Where next:**

- [Policy & routing](/policy-routing/index.md) — [selectors](/glossary/index.md#selector), [actions](/glossary/index.md#action), [pools](/glossary/index.md#pool), [retries](/glossary/index.md#retry), and Rhai for rules ([Rule Rhai](/glossary/index.md#rule-rhai))
- [Rhai](/rhai/index.md) — Rhai for rules and Rhai for processor chains
- [Processor chains](/processor-chains/index.md) — planned `processors:` wire editing (**not yet shipped**)
- [Planned plugin models](/concepts/planned-plugin-models.md) — optional read: [WASM](/glossary/index.md#wasm), [sidecar](/glossary/index.md#sidecar), processor-chain roadmap
- [Observability](/observability/index.md) — [metrics](/observability/metrics.md), tracing, and event export
- [Glossary](/glossary/index.md) — short definitions for terms used across the docs
