---
toc_depth: 3
---

# Data sources

**`data_sources:`** is an optional top-level config list of named, read-only tables and views that Conduit loads into the [runtime snapshot](/glossary/index.md#runtime-snapshot) at compile/reload time. Two subsystems consume them: [Client ACLs](/policy-routing/client-acls.md) match client IPs against **`type: cidr`** views, and [Rhai lookups](/rhai/data-sources-and-lookups.md) read tables with **`lookup(table, key)`** and **`lookup_ip(name, addr)`** in rule scripts. This page owns the config, file formats, load-safety limits, and reload behavior shared by both; each consumer page covers how it uses the data.

These tables are **not** the [Lookup](/concepts/architecture-and-packet-path.md#lookup) pipeline phase — see [Glossary — Lookup vs lookup(table, key)](/glossary/index.md#lookup-vs-lookuptable-key). Conduit reads the files at snapshot build; scripts and ACL cannot open arbitrary paths.

## Overview

| Piece | Role |
|-------|------|
| **`data_sources:`** | Top-level config list — each entry names one table/view and points at a file |
| **`type: csv`** | Exact-key table; read by Rhai **`lookup(table, key)`** |
| **`type: cidr`** | Longest-prefix IPv4/IPv6 view; used by [Client ACLs](/policy-routing/client-acls.md) and Rhai **`lookup_ip(name, addr)`** |
| **Snapshot compile** | Files read and parsed when config validates/builds; failures block reload |

**Grant model:** only the **`name`** values you list under **`data_sources:`** are visible to scripts and ACL. There is no path-based or implicit discovery — a script or ACL rule can only reach a table that config names.

## Consumers

| Consumer | Uses | How |
|----------|------|-----|
| [Client ACLs](/policy-routing/client-acls.md) | **`type: cidr`** | Host gate matches the client socket IP against a named view (no scripting) |
| [Rhai lookups](/rhai/data-sources-and-lookups.md) | **`type: csv`** and **`type: cidr`** | Rule scripts call **`lookup`** / **`lookup_ip`** on the request or response hook |

## Config schema

Field reference: [Config schema: data sources](/reference/config-schema/data-sources.md) (`data_sources:` and `data_source_limits:`).

Each entry in **`data_sources:`**:

| Field {: .column-no-wrap } | Required | Default | Meaning |
|-------|----------|---------|---------|
| `name` | yes | — | Name passed to **`lookup`** / **`lookup_ip`** / ACL **`match`** (must be unique) |
| `type` | yes | — | **`csv`** or **`cidr`** |
| `path` | yes | — | File path; resolved relative to the [config file directory](/control-plane/config-file.md#path-resolution-base-directory) unless absolute |
| `key_column` | no | `key` (or first column) | **`csv` only** — header name for the key column, or use column index 0 when no header |
| `value_column` | no | `value` (or second column) | **`csv` only** — header name for the value column, or use column index 1 when no header |
| `max_file_bytes` | no | inherit `data_source_limits` | Per-entry override of the source file size cap (see [Load-safety limits](#load-safety-limits)) |
| `max_entries` | no | inherit `data_source_limits` | Per-entry override of the entry/prefix cap |
| `max_key_bytes` | no | inherit `data_source_limits` | Per-entry override of the per-key length cap (**`csv`**) |
| `max_value_bytes` | no | inherit `data_source_limits` | Per-entry override of the per-value length cap |

```yaml
data_sources:
  - name: blocklist
    type: csv
    path: data/blocklist.csv
    key_column: qname
    value_column: action
  - name: corp_nets
    type: cidr
    path: data/corp_nets.txt
```

## Source types

### CSV format { #csv-format }

Conduit uses a **simple** comma-separated parser for **`type: csv`** tables:

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

### CIDR sources { #cidr-sources }

```yaml
data_sources:
  - name: corp_nets
    type: cidr
    path: data/corp_nets.txt
```

File format: one prefix per line (`10.0.0.0/8`, `2001:db8::/32`, or a bare host IP). Optional trailing value after whitespace. Comments (`#`) and blank lines are ignored. IPv4 and IPv6 are first-class, and the **most-specific** matching prefix wins.

A line with no value is stored as a membership marker, so a hit is always a non-empty string. [Client ACLs](/policy-routing/client-acls.md) treat any hit as membership even when the value is empty; Rhai **`lookup_ip`** returns the value string on a hit and **`""`** on a miss.

Example `corp_nets.txt`:

```text
# corporate networks
10.0.0.0/8       corp
2001:db8::/32    corp
192.0.2.5        jump-host
```

## Load-safety limits { #load-safety-limits }

Data-source tables load **entirely into memory** at snapshot compile (validate, reload, apply). To keep an oversized or malformed table file from driving Conduit into high memory use or OOM, the loader enforces generic load-safety limits. The limits are framed on the **table / entry abstraction**, so they apply to **`csv`** and **`cidr`** sources alike.

Set a global **`data_source_limits:`** block; any field left unset (or `0`) uses the built-in default. Individual entries may override the per-table caps via the fields in the table above.

| Limit | Scope | Default | Bounds |
|-------|-------|---------|--------|
| `max_file_bytes` | per table (overridable) | 16 MiB | Source file size; read is bounded, oversize is rejected without loading the whole file |
| `max_entries` | per table (overridable) | 1,000,000 | Number of entries (CSV rows or CIDR prefixes) loaded from one table |
| `max_key_bytes` | per table (overridable) | 1 KiB | Length of any single **`csv`** key |
| `max_value_bytes` | per table (overridable) | 4 KiB | Length of any single value |
| `max_tables` | global | 64 | Number of entries in `data_sources:` |
| `max_total_bytes` | global | 64 MiB | Aggregate bytes read across all tables in one snapshot |

Resolution precedence for the per-table caps: a non-zero **per-entry override** wins, otherwise the **`data_source_limits`** value, otherwise the **built-in default**. **`max_key_bytes`** applies to **`csv`** keys only — **`cidr`** prefixes are parsed as networks, not free-form keys.

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

In-memory tables are intended for blocklists, geo maps, CIDR nets, and similar policy data — **not** a bulk data store. For large or streaming datasets, keep tables within these caps rather than raising them indefinitely.

## Reload and snapshot { #reload-and-snapshot }

| Event | Effect on tables |
|-------|------------------|
| **`conduitctl validate --file`** | Reads source files and compiles scripts — same load path as runtime |
| **Config reload** (new snapshot generation) | Files re-read; ACL and **`lookup`** / **`lookup_ip`** use the new in-memory tables for **new** queries |
| **In-flight transaction** | Keeps the snapshot generation it started with until the transaction completes |
| **API overlay** | Non-empty overlay **`data_sources`** list **replaces** the file-layer list — see [Configuration model](/control-plane/configuration-model.md) |

If a source path is missing or malformed at compile time, validation/reload **fails** and the previous snapshot remains active.

## Validation errors

| Message (substring) | Cause |
|---------------------|--------|
| `data_sources entry name must not be empty` | Missing `name` |
| `duplicate data_sources name` | Two entries share the same `name` |
| `unsupported type` | `type` is not `csv` or `cidr` |
| `path must not be empty` | Missing `path` |
| `failed to read` | File not found or not readable at compile time |
| `not enough columns` | CSV row has fewer columns than key/value indices require |
| `invalid CIDR prefix` | A **`cidr`** line is not a valid prefix or address |
| `exceeds max_file_bytes` | Source file is larger than the effective `max_file_bytes` cap |
| `exceeds max_entries` | Table has more entries than the effective `max_entries` cap |
| `exceeds max_key_bytes` / `exceeds max_value_bytes` | A key or value is longer than the effective cap |
| `exceeds max_tables` | More `data_sources:` entries than `data_source_limits.max_tables` |
| `exceeds max_total_bytes` | Aggregate bytes across all tables exceed `data_source_limits.max_total_bytes` |
| `unknown data source` | A Rhai script calls `lookup("literal", …)` / `lookup_ip("literal", …)` with a name not listed in **`data_sources:`** |

Structural YAML checks run in **`validate`** (including the `max_tables` count). File read and parse — and the file/entry/cell/aggregate byte limits — run when the snapshot is **built** (including `conduitctl validate` with script compile).

## Limitations (current release)

- **`type: csv`** and **`type: cidr`** only — no HTTP, LMDB, or dynamic plugins yet.
- Tables load fully into memory and are bounded by the [load-safety limits](#load-safety-limits) — intended for policy data, not bulk storage.
- Simple CSV splitting — no RFC 4180 quoting; avoid commas inside fields.
- Read-only from scripts and ACL — no write-back or per-query cache invalidation.

## Related topics

- [Client ACLs](/policy-routing/client-acls.md) — `type: cidr` views for host client-IP policy
- [Rhai lookups](/rhai/data-sources-and-lookups.md) — `lookup()` / `lookup_ip()` in rule scripts
- [Config schema: data sources](/reference/config-schema/data-sources.md) — field tables for `data_sources:` and `data_source_limits:`
- [Config file](/control-plane/config-file.md) — top-level blocks and path resolution
- [Reload and export](/control-plane/reload-and-export.md) — what reload updates
