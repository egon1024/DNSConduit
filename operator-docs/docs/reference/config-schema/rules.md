# Reference: rules

Schema reference for the top-level `rules:` block. Operator-oriented behavior: [Rules and actions](/policy-routing/rules-and-actions.md).

## `rules`

| Field | Type | Required | Default | Meaning |
|-------|------|----------|---------|---------|
| `match_mode` | string | no | `first_match` | Rule evaluation mode. Only **`first_match`** is supported today; other modes may be added in a future release. |
| `rules` | list of rule objects | no | `[]` | Ordered list of rules |

## Rule object

| Field | Type | Required | Meaning |
|-------|------|----------|---------|
| `name` | string | yes | Unique rule name (non-empty) |
| `hook` | string | yes | `request` or `response` |
| `selectors` | list | no | Match conditions; empty list matches all queries on this hook |
| `actions` | list | yes | Built-in [actions](/glossary/index.md#action); run in **list order** |

## Selector object

| Field | Type | Required | Meaning |
|-------|------|----------|---------|
| `type` | string | yes | Selector type (see groups below) |
| `value` | string | yes | Selector-specific match string |

**Query identity:** `qname_exact`, `qname_suffix`, `qtype`

**Response outcome:** `rcode`

**Transaction metadata:** `tag`

**Sampling and cadence:** `every_nth_global`, `every_nth_worker`, `sample_percent` — see [Rules and actions — Sampling and cadence](/policy-routing/rules-and-actions.md#sampling-and-cadence)

`sample_percent` expects a float in **`[0, 100]`**.

`every_nth_worker` and `every_nth_global` expect an integer **`N >= 1`**.

Full operator-oriented grouping: [Rules and actions — Selectors](/policy-routing/rules-and-actions.md#selectors).

## Action object

| Field | Type | Required | Meaning |
|-------|------|----------|---------|
| `type` | string | yes | Action name (see below) |
| `value` | string | varies | Action argument |

### Action types

| `type` | Valid hooks | `value` |
|--------|-------------|---------|
| `set_pool` | request | Pool name |
| `set_tag` | request, response | `key=value` or `key` |
| `set_source_v4` | **request only** | IPv4 address in configured `sources_v4` union |
| `set_source_v6` | **request only** | IPv6 address in configured `sources_v6` union |
| `retry` | response | [Retry](/glossary/index.md#retry) in the current [pool](/glossary/index.md#pool) (`value` ignored) |
| `retry_pool` | response | Pool name — one-shot [retry](/glossary/index.md#retry) target (`value` required) |
| `set_rcode` | response | RCODE name (for example `SERVFAIL`) |
| `drop` | request, response | Ignored |
| `rhai` | request, response | Path to `.rhai` script (non-empty) |

### `set_source_v4` / `set_source_v6` validation

At config load / validate:

- **`value`** must be a valid IPv4 or IPv6 address.
- The address must appear in the union of **`forward.sources_v4`** / **`forward.sources_v6`** and every pool’s **`sources_v4`** / **`sources_v6`**.
- At least one corresponding source list must be non-empty.

At [Forward](/concepts/architecture-and-packet-path.md#forward), the override must be allowed for the **selected pool**; otherwise Conduit falls back to round-robin source selection (same as [Rhai](/rhai/index.md)).

See [Rules and actions — Action order](/policy-routing/rules-and-actions.md#action-order-on-one-rule).
