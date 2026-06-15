# Config schema: events

Field reference for the top-level **`events:`** block and each [event sink](/glossary/index.md#event-sink). For behavior — dnstap export, filters, overload, and lab validation — see [Event export](/observability/event-export.md).

## `events`

| Property | Value |
|----------|--------|
| **Type** | Object |
| **Required** | No — when omitted, no sinks are configured and export is off |
| **Location** | Top-level key in the [config file](/control-plane/config-file.md) or [overlay](/glossary/index.md#overlay) (whole-section replace) |

| Field {: .column-no-wrap } | Type | Required | Default | Description |
|-------|------|----------|---------|-------------|
| `queue_depth` | integer | no | **4096** | Per-sink queue capacity. Must be **≥ 1** when the `events:` block is present |
| `drop_policy` | string | no | **`drop_oldest`** | **`drop_oldest`** or **`drop_newest`** on queue overflow |
| `sinks` | list | no | `[]` | [Event sink](#event-sink-object) definitions. Export is enabled only when at least one valid sink compiles |

## Event sink object

Each entry under `events.sinks` describes one dnstap export path.

| Field {: .column-no-wrap } | Type | Required | Default | Description |
|-------|------|----------|---------|-------------|
| `type` | string | yes | — | Must be **`dnstap`** (only supported value today) |
| `name` | string | conditional | — | Canonical operator / metrics id. Required unless `export_id` is set (legacy) |
| `export_id` | string | conditional | `name` | dnstap protobuf wire identity. Defaults to `name` when `name` is set and `export_id` is empty |
| `destinations` | list of strings | yes | — | At least one **`unix:`** or **`tcp:`** destination (see [Event export — Destinations](/observability/event-export.md#destinations)) |
| `emit` | list of strings | no | **`query`**, **`response`** | **`query`**, **`response`**, **`retry`** — which observation points export frames |
| `filters` | object | no | (no filtering) | [Sink filters](#sink-filters-object) |
| `extra_fields` | list of strings | no | `[]` | Metadata keys embedded in dnstap **`extra`** JSON |
| `extra_tags` | list of strings | no | (all tags) | Tag key filter when `tags` is in `extra_fields`; `*` = all tags |
| `connect_retry` | object | no | (see below) | Backoff when destinations are unreachable |

### Allowed `extra_fields` values

`pool`, `backend`, `attempt_count`, `txn_id`, `qname`, `rcode`, `tags`, `client`, `sink_name`

Unknown names fail validation.

### Sink filters object

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `tag_required` | string | — | Transaction must have this tag key |
| `selectors` | list | `[]` | [Selector](/glossary/index.md#selector) objects (`type`, `value`); same types as [rules](/reference/config-schema/rules.md). All must match |
| `sample_rate` | float | **1.0** | Must be in **(0, 1]**; deterministic per-transaction sampling |
| `pool` | string | — | Match selected pool (`response` / `retry` only) |
| `backend` | string | — | Match selected backend address (`response` / `retry` only) |

### `connect_retry` object

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `initial_ms` | integer | **1000** | First retry delay (ms); must be **> 0** when set |
| `max_ms` | integer | **30000** | Maximum delay cap (ms) |
| `multiplier` | float | **2.0** | Exponential factor; must be **≥ 1.0** |
| `max_elapsed_ms` | integer | **0** | Stop retrying after this many ms (**0** = unlimited) |
| `jitter` | boolean | **true** | Randomize delay within the computed window |

## Validation summary

| Rule | Error if violated |
|------|-------------------|
| Each sink has `name` and/or non-empty `export_id` | `requires name or export_id` |
| Unique `name` across sinks | `name '…' duplicates sinks[…]` |
| Unique `export_id` across sinks | `export_id '…' duplicates sinks[…]` |
| `type: dnstap` with ≥ 1 parseable destination | Sink skipped at compile if invalid (no export for that entry) |
| `queue_depth` ≥ 1 when `events:` present | `events.queue_depth must be >= 1` |
| Valid `drop_policy` | Unrecognized values default to **`drop_oldest`** at compile time |
| Valid `extra_fields` names | `unknown events extra_fields entry '…'` |
| `extra_tags` without `tags` in `extra_fields` | `extra_tags requires 'tags' in extra_fields` |
| `extra_tags` cannot mix `*` with other keys | validation error |
| `sample_rate` in (0, 1] | `sample_rate must be in (0, 1]` |
| Valid selector `type` in filters | `unknown selector type '…'` |

Validate with `conduitctl validate --file …` or load via the running process; see [Config file](/control-plane/config-file.md).

## Example configuration

```yaml
events:
  queue_depth: 8192
  drop_policy: drop_oldest
  sinks:
    - type: dnstap
      name: prod-tap
      export_id: conduit-prod-1
      destinations:
        - "unix:/var/run/dnstap.sock"
        - "tcp:127.0.0.1:6000"
      emit:
        - query
        - response
        - retry
      filters:
        sample_rate: 0.25
        selectors:
          - type: qname_suffix
            value: "corp.example."
      extra_fields:
        - pool
        - backend
        - tags
      extra_tags:
        - tenant
        - vip
      connect_retry:
        initial_ms: 500
        max_ms: 15000
        multiplier: 2.0
        jitter: true
```

## Related topics

- [Event export](/observability/event-export.md) — operator guide
- [Reference: rules](/reference/config-schema/rules.md) — selector types shared with sink filters
- [Configuration model](/control-plane/configuration-model.md) — overlay and reload semantics
- [Built-in metrics — Event export](/observability/built-in-metrics.md#event-export) — per-sink counters
