# Reference: config schema

Field-level reference for the Conduit YAML **`schema_version: 1`** document. Syntax, path resolution, and validation behavior: [Config file](/control-plane/config-file.md). Conceptual model: [Configuration model](/control-plane/configuration-model.md).

| Section | Reference page |
|---------|----------------|
| `listeners` | [Listeners](/reference/config-schema/listeners.md) |
| `pools` | [Pools](/reference/config-schema/pools.md) |
| `rules` | [Rules](/reference/config-schema/rules.md) |
| `control` | [Control](/reference/config-schema/control.md) |
| `events` | [Events](/reference/config-schema/events.md) |
| `metrics`, `tracing` | [Metrics and tracing](/reference/config-schema/metrics-and-tracing.md) |

Other top-level blocks (`forward`, `orchestrator`, `rhai`, `logging`, `data_sources`) are documented in progress on their topic pages; see [Config file — top-level blocks](/control-plane/config-file.md).

Canonical machine-readable schema: `proto/conduit/v1/config.proto`.
