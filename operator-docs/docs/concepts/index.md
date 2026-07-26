# Concepts

This section covers mental models for how Conduit handles DNS — the query path on the [dataplane](/glossary/index.md#dataplane) and where policy plugs in. Read these before diving into config field lists and runbooks; YAML syntax and operational detail are described in other sections.

**Read in order:**

1. [Architecture and packet path](/concepts/architecture-and-packet-path.md) — [listeners](/glossary/index.md#listener), [pipeline phases](/concepts/architecture-and-packet-path.md#pipeline-phases), [transactions](/glossary/index.md#transaction), [runtime snapshot](/glossary/index.md#runtime-snapshot), and [how a query reaches upstream and back](/concepts/architecture-and-packet-path.md#end-to-end-path)
2. [Runtime and concurrency](/concepts/runtime-and-concurrency.md) — dataplane runtime models (`sync` / `split_io`), worker pools, the [transaction](/glossary/index.md#transaction) slot pool, and graceful shutdown drain

**Where next:**

- [Policy & routing](/policy-routing/index.md) — [selectors](/glossary/index.md#selector), [actions](/glossary/index.md#action), [pools](/glossary/index.md#pool), [backend health](/policy-routing/backend-health.md), [retries](/glossary/index.md#retry), and Rhai for rules ([Rule Rhai](/glossary/index.md#rule-rhai))
- [Rhai](/rhai/index.md) — Rhai for rules
- [Observability](/observability/index.md) — [metrics](/observability/metrics.md), tracing, and event export
- [Glossary](/glossary/index.md) — short definitions for terms used across the docs
