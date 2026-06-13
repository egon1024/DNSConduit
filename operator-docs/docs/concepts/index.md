# Concepts

Mental models for how Conduit handles DNS — the query path on the [dataplane](/glossary/index.md#dataplane) and where policy plugs in. Read these before diving into config field lists and runbooks; YAML syntax and operational detail live in other sections.

**Read in order:**

1. [Architecture and packet path](/concepts/architecture-and-packet-path.md) — listeners, [pipeline phases](/concepts/architecture-and-packet-path.md#pipeline-phases), [transactions](/glossary/index.md#transaction), [runtime snapshot](/glossary/index.md#runtime-snapshot), and how a query reaches upstream and back
2. [Extensibility](/concepts/extensibility.md) — built-in [rules](/policy-routing/rules-and-actions.md), [Rhai](/glossary/index.md#rhai), and planned plugin models on the request and response hooks (after you understand the pipeline)

**Where next:**

- [Policy & routing](/policy-routing/rules-and-actions.md) — [selectors](/glossary/index.md#selector), [actions](/glossary/index.md#action), [pools](/glossary/index.md#pool), and [retries](/glossary/index.md#retry)
- [Observability](/observability/index.md) — [metrics](/observability/metrics.md), tracing, and event export
- [Glossary](/glossary/index.md) — short definitions for terms used across the docs
