# Reference: config schema

Field-level reference for the Conduit YAML **`schema_version: 1`** document. Syntax, path resolution, and validation behavior: [Config file](/control-plane/config-file.md). Conceptual model: [Configuration model](/control-plane/configuration-model.md).

| Section | Reference page |
|---------|----------------|
| `listeners` | [Listeners](/reference/config-schema/listeners.md) |
| `pools` | [Pools](/reference/config-schema/pools.md) |
| `pools[].health` | [Health](/reference/config-schema/health.md) |
| `forward` | [Forward](/reference/config-schema/forward.md) |
| `orchestrator` | [Orchestrator](/reference/config-schema/orchestrator.md) |
| `dataplane` | [Dataplane](/reference/config-schema/dataplane.md) |
| `shutdown` | [Shutdown](/reference/config-schema/shutdown.md) |
| `rules` | [Rules](/reference/config-schema/rules.md) |
| `control` | [Control](/reference/config-schema/control.md) |
| `events` | [Events](/reference/config-schema/events.md) |
| `metrics`, `tracing` | [Metrics and tracing](/reference/config-schema/metrics-and-tracing.md) |

Other top-level blocks — topic home (field tables and behavior):

| Section | Topic page |
|---------|------------|
| `rhai` | [Sandbox limits](/rhai/sandbox-limits.md) — `max_operations`, `max_call_depth`, `hook_timeout_ms` |
| `data_sources` | [Data sources and lookups](/rhai/data-sources-and-lookups.md) |
| `data_source_limits` | [Data sources — Load-safety limits](/rhai/data-sources-and-lookups.md#load-safety-limits) — `max_file_bytes`, `max_entries`, `max_key_bytes`, `max_value_bytes`, `max_tables`, `max_total_bytes` |
| `logging` | [Logging](/observability/logging.md) — `level`, `output` |

See [Config file — top-level blocks](/control-plane/config-file.md#top-level-blocks) for how blocks fit together.

Canonical machine-readable schema: `proto/conduit/v1/config.proto`.
