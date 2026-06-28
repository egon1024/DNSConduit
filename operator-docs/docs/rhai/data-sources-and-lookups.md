---
toc_depth: 3
---

# Data sources and lookups

Lookup tables declared under **`data_sources:`** in config supply read-only data for **`table_lookup(table, key)`** in [Rhai for rules](/rhai/rule-rhai.md) scripts. Conduit loads tables into the [runtime snapshot](/glossary/index.md#runtime-snapshot) at compile/reload time; scripts cannot open arbitrary files.

API reference card: [Transaction API — Lookups](/rhai/transaction-api.md#lookups). Hook availability: [Hooks and phases — phase guards](/rhai/hooks-and-phases.md#phase-guards).

## Overview

| Piece | Role |
|-------|------|
| **`data_sources:`** | Top-level config list — each entry names one table and points at a CSV file |
| **`table_lookup(table, key)`** | Rhai function — returns the value string, or **`""`** on miss |
| **Snapshot compile** | CSV read and parsed when config validates/builds; failures block reload |

**Grant model:** only **`name`** values you list under **`data_sources:`** are visible to scripts. There is no path-based or implicit discovery.

## Config schema

Each entry in **`data_sources:`**:

| Field {: .column-no-wrap } | Required | Default | Meaning |
|-------|----------|---------|---------|
| `name` | yes | — | Table name passed to **`table_lookup`** (must be unique in the list) |
| `type` | yes | — | **`csv`** only in current releases |
| `path` | yes | — | CSV file path; resolved relative to the [config file directory](/control-plane/config-file.md#path-resolution-base-directory) unless absolute |
| `key_column` | no | `key` (or first column) | Header name for the key column, or use column index 0 when no header |
| `value_column` | no | `value` (or second column) | Header name for the value column, or use column index 1 when no header |
| `max_file_bytes` | no | inherit `data_source_limits` | Per-entry override of the source file size cap (see [Load-safety limits](#load-safety-limits)) |
| `max_entries` | no | inherit `data_source_limits` | Per-entry override of the key→value entry cap |
| `max_key_bytes` | no | inherit `data_source_limits` | Per-entry override of the per-key length cap |
| `max_value_bytes` | no | inherit `data_source_limits` | Per-entry override of the per-value length cap |

Minimal example:

```yaml
data_sources:
  - name: blocklist
    type: csv
    path: data/blocklist.csv
    key_column: qname
    value_column: action
```

With rules:

```yaml
rules:
  match_mode: first_match
  rules:
    - name: blocklist-check
      hook: request
      selectors:
        - type: qname_suffix
          value: ".example."
      actions:
        - type: rhai
          value: scripts/blocklist.rhai
```

## Load-safety limits { #load-safety-limits }

Data-source tables load **entirely into memory** at snapshot compile (validate, reload, apply). To keep an oversized or malformed table file from driving Conduit into high memory use or OOM, the loader enforces generic load-safety limits. The limits are framed on the **table / key→value abstraction**, so they apply to any source type (CSV is the only type today).

Set a global **`data_source_limits:`** block; any field left unset (or `0`) uses the built-in default. Individual entries may override the per-table caps via the fields in the table above.

| Limit | Scope | Default | Bounds |
|-------|-------|---------|--------|
| `max_file_bytes` | per table (overridable) | 16 MiB | Source file size; read is bounded, oversize is rejected without loading the whole file |
| `max_entries` | per table (overridable) | 1,000,000 | Number of key→value entries loaded from one table |
| `max_key_bytes` | per table (overridable) | 1 KiB | Length of any single key |
| `max_value_bytes` | per table (overridable) | 4 KiB | Length of any single value |
| `max_tables` | global | 64 | Number of entries in `data_sources:` |
| `max_total_bytes` | global | 64 MiB | Aggregate bytes read across all tables in one snapshot |

Resolution precedence for the per-table caps: a non-zero **per-entry override** wins, otherwise the **`data_source_limits`** value, otherwise the **built-in default**.

```yaml
data_source_limits:
  max_file_bytes: 33554432   # 32 MiB
  max_entries: 2000000
  max_tables: 16
data_sources:
  - name: blocklist
    type: csv
    path: data/blocklist.csv
    key_column: qname
    value_column: action
    max_entries: 5000        # this table is small; tighten its own cap
```

In-memory tables are intended for blocklists, geo maps, and similar policy data — **not** a bulk data store. For large or streaming datasets, keep tables within these caps rather than raising them indefinitely.

## CSV format

Conduit uses a **simple** comma-separated parser:

- One row per line; fields split on **`,`** with surrounding whitespace trimmed.
- **No** quoted-field / embedded-comma support — keep keys and values free of commas.
- Blank lines and lines starting with **`#`** are ignored.
- Optional header row: detected when a column equals **`key_column`** / **`value_column`**, or the first column is **`qname`** or **`key`** (case-insensitive). The header row is not loaded as data.
- Without a detected header, column **0** is the key and column **1** is the value.

Example `blocklist.csv`:

```csv
qname,action
bad.example.,block
good.example.,allow
```

Example `geo.csv`:

```csv
qname,region
eu.example.,eu
us.example.,us
```

**Duplicate keys:** if the same key appears on multiple rows, the **last** row wins.

**Key matching:** lookups are exact string equality. For DNS names, match how clients send the qname (often FQDN with trailing dot: `bad.example.`).

## `table_lookup` behavior

```rhai
let action = table_lookup("blocklist", question_qname(txn));
```

| Situation | Return value | Observability |
|-----------|----------------|---------------|
| Key found | Value cell as string | — |
| Key not in table | `""` | Silent (expected miss) |
| Unknown `table` name (not in `data_sources:`) | `""` | Warn log (milestone + periodic) and [`conduit_script_errors_total`](/observability/built-in-metrics.md#conduit_script_errors_total) (`reason="lookup_unknown_table"`) |
| Empty value cell | `""` (still a “hit” if key exists — rare in practice) | — |

**Compile-time check:** when the table argument is a **string literal** in Rhai source (for example `table_lookup("blocklist", …)`), Conduit validates the name against **`data_sources:`** at snapshot build. A typo fails **`conduitctl validate`** and reload. Dynamic table names (variable or expression) are not checked at compile time — they surface at runtime with the observability row above.

Use **`question_qname(txn)`** or **`txn.question().qname`** for qname-keyed tables. On the [response hook](/rhai/hooks-and-phases.md#response-hook), the question is unchanged from the client query.

Counts toward [sandbox limits](/rhai/sandbox-limits.md) (`rhai.max_operations`, `rhai.hook_timeout_ms`) like other host calls.

## Reload and snapshot { #reload-and-snapshot }

| Event | Effect on lookups |
|-------|-------------------|
| **`conduitctl validate --file`** | Reads CSVs and compiles scripts — same load path as runtime |
| **Config reload** (new snapshot generation) | CSV files re-read; **`table_lookup`** uses new in-memory tables for **new** queries |
| **In-flight transaction** | Keeps the snapshot generation it started with until the transaction completes |
| **API overlay** | Non-empty overlay **`data_sources`** list **replaces** the file-layer list — see [Configuration model](/control-plane/configuration-model.md) |

If a CSV path is missing or malformed at compile time, validation/reload **fails** and the previous snapshot remains active.

## Validation errors

| Message (substring) | Cause |
|---------------------|--------|
| `data_sources entry name must not be empty` | Missing `name` |
| `duplicate data_sources name` | Two entries share the same `name` |
| `unsupported type` | `type` is not `csv` |
| `path must not be empty` | Missing `path` |
| `failed to read` | File not found or not readable at compile time |
| `not enough columns` | Row has fewer columns than key/value indices require |
| `exceeds max_file_bytes` | Source file is larger than the effective `max_file_bytes` cap |
| `exceeds max_entries` | Table has more key→value entries than the effective `max_entries` cap |
| `exceeds max_key_bytes` / `exceeds max_value_bytes` | A key or value cell is longer than the effective cap |
| `exceeds max_tables` | More `data_sources:` entries than `data_source_limits.max_tables` |
| `exceeds max_total_bytes` | Aggregate bytes across all tables exceed `data_source_limits.max_total_bytes` |
| `unknown data source` | Rhai script calls `table_lookup("literal", …)` with a table name not listed in **`data_sources:`** |

Structural YAML checks run in **`validate`** (including the `max_tables` count). File read and CSV parse — and the file/entry/cell/aggregate byte limits — run when the snapshot is **built** (including `conduitctl validate` with script compile).

## Patterns

### Blocklist (request hook)

Script `blocklist.rhai`:

```rhai
if table_lookup("blocklist", question_qname(txn)) == "block" {
    txn.drop_query_now();
}
```

Config: inline **`conduit.yaml`** beside the CSV — see [Rhai policy — Blocklist drop](/guides/rhai-policy.md#example-1-blocklist-drop-request-hook).

### Tag for observability

Script `lookup-demo.rhai`:

```rhai
let region = table_lookup("geo", question_qname(txn));
if region != "" {
    txn.set_tag("region", region);
}
```

Downstream sinks can filter on **`tag_required`** or rules can branch on **`txn.has_tag("region")`**. Walkthrough: [Event export and dnstap — Tag-gated export](/guides/event-export-dnstap.md#5-optional-checks).

### Pool or egress map

```rhai
let pool = table_lookup("routing", question_qname(txn));
if pool != "" {
    txn.set_pool(pool);
}
```

List **`set_pool`** before **`rhai`** on the same rule when the script only refines pool choice and built-ins must run first — see [Action order on one rule](/policy-routing/rules-and-actions.md#action-order-on-one-rule).

## Runnable examples

| Script | In repository fixtures? | What it shows | Walkthrough |
|--------|-------------------------|----------------|-------------|
| `blocklist.rhai` | Yes (`tests/fixtures/rhai/`) | Drop on CSV `block` | [Rhai policy — Blocklist](/guides/rhai-policy.md#example-1-blocklist-drop-request-hook) |
| `route-by-table.rhai` | Guide only (copy from lab YAML) | CSV qname → pool | [Rhai policy — CSV pool routing](/guides/rhai-policy.md#example-2-csv-pool-routing-request-hook) |
| `lookup-demo.rhai` | Yes | Grant model — only configured tables | [Data sources — Tag for observability](#tag-for-observability) |
| `block-hits.rhai` | Yes | Lookup + user metrics | [User metrics](/rhai/user-metrics.md) |

## Limitations (current release)

- **`type: csv`** only — no HTTP, LMDB, or dynamic plugins yet.
- Tables load fully into memory and are bounded by [load-safety limits](#load-safety-limits) — intended for policy data, not bulk storage.
- Simple CSV splitting — no RFC 4180 quoting; avoid commas inside fields.
- Read-only from scripts — no write-back or per-query cache invalidation.
- [Rhai for rules](/rhai/rule-rhai.md) only — `table_lookup` is available to rule scripts on the request and response hooks.

## Related topics

- [Transaction API — Lookups](/rhai/transaction-api.md#lookups) — `table_lookup` card
- [Rhai for rules](/rhai/rule-rhai.md) — when to use scripts vs built-in selectors
- [Config file](/control-plane/config-file.md) — top-level blocks and path resolution
- [Reload and export](/control-plane/reload-and-export.md) — what reload updates
- [Sandbox limits](/rhai/sandbox-limits.md) — operation and timeout caps
- [Event export](/observability/event-export.md) — tags set from lookup results
