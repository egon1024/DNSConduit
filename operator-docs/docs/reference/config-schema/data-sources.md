# Config schema: data sources

Field reference for the optional top-level **`data_sources:`** list and **`data_source_limits:`** block. For CSV format, `lookup(table, key)` behavior, patterns, and reload semantics, see [Data sources and lookups](/rhai/data-sources-and-lookups.md).

These tables feed **`lookup(table, key)`** in Rule Rhai — not the [Lookup](/concepts/architecture-and-packet-path.md#lookup) pipeline phase. See [Glossary — Lookup vs lookup(table, key)](/glossary/index.md#lookup-vs-lookuptable-key).

## `data_sources`

| Property | Value |
|----------|--------|
| **Type** | List of objects |
| **Required** | No — when omitted, no tables are available to scripts |
| **Location** | Top-level key in the [config file](/control-plane/config-file.md) or [overlay](/glossary/index.md#overlay) (non-empty overlay list **replaces** the file-layer list) |

Each entry:

| Field {: .column-no-wrap } | Type | Required | Default | Description |
|-------|------|----------|---------|-------------|
| `name` | string | yes | — | Table name passed to **`lookup`** (must be unique in the list) |
| `type` | string | yes | — | **`csv`** only |
| `path` | string | yes | — | CSV file path; resolved relative to the [config file directory](/control-plane/config-file.md#path-resolution-base-directory) unless absolute |
| `key_column` | string | no | `key` (or first column) | Header name for the key column, or column index 0 when no header |
| `value_column` | string | no | `value` (or second column) | Header name for the value column, or column index 1 when no header |
| `max_file_bytes` | integer | no | inherit `data_source_limits` | Per-entry override of the source file size cap |
| `max_entries` | integer | no | inherit `data_source_limits` | Per-entry override of the key→value entry cap |
| `max_key_bytes` | integer | no | inherit `data_source_limits` | Per-entry override of the per-key length cap |
| `max_value_bytes` | integer | no | inherit `data_source_limits` | Per-entry override of the per-value length cap |

```yaml
data_sources:
  - name: blocklist
    type: csv
    path: data/blocklist.csv
    key_column: qname
    value_column: action
```

## `data_source_limits`

| Property | Value |
|----------|--------|
| **Type** | Object |
| **Required** | No — unset or **`0`** fields use built-in defaults |
| **Location** | Top-level key in the [config file](/control-plane/config-file.md) |

| Field {: .column-no-wrap } | Scope | Default | Description |
|-------|-------|---------|-------------|
| `max_file_bytes` | per table (overridable) | **16 MiB** | Source file size; oversize is rejected without loading the whole file |
| `max_entries` | per table (overridable) | **1,000,000** | Key→value entries loaded from one table |
| `max_key_bytes` | per table (overridable) | **1 KiB** | Length of any single key |
| `max_value_bytes` | per table (overridable) | **4 KiB** | Length of any single value |
| `max_tables` | global | **64** | Number of entries in `data_sources:` |
| `max_total_bytes` | global | **64 MiB** | Aggregate bytes read across all tables in one snapshot |

For per-table caps, a non-zero **per-entry override** wins, otherwise the **`data_source_limits`** value, otherwise the **built-in default**. Detail: [Load-safety limits](/rhai/data-sources-and-lookups.md#load-safety-limits).

```yaml
data_source_limits:
  max_file_bytes: 33554432
  max_entries: 2000000
  max_tables: 16
data_sources:
  - name: blocklist
    type: csv
    path: data/blocklist.csv
    max_entries: 5000
```

## Validation summary

| Rule | Typical failure |
|------|-----------------|
| Unique `name` | Duplicate table name |
| `type: csv` | Unsupported type |
| Readable `path` within size caps | Missing file or `exceeds max_file_bytes` |
| Within `max_tables` / `max_total_bytes` | `exceeds max_tables` / `exceeds max_total_bytes` |
| Entry / key / value size caps | `exceeds max_entries` / key or value length errors |

Full error list: [Data sources — Validation errors](/rhai/data-sources-and-lookups.md#validation-errors). Tables load at snapshot compile; failures reject the new snapshot.

## Related topics

- [Data sources and lookups](/rhai/data-sources-and-lookups.md) — CSV, `lookup()`, patterns, reload
- [Config schema: rhai](/reference/config-schema/rhai.md) — sandbox limits
- [Config file — path resolution](/control-plane/config-file.md#path-resolution-base-directory)
- [Configuration model](/control-plane/configuration-model.md)
