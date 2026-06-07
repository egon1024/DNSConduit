# Config schema: pools

Field reference for the top-level `pools:` list and each [pool](/glossary/index.md#pool) / [backend](/glossary/index.md#backend) object. For behavior — selection, weights, multiple pools, retries — see [Pools and backends](/policy-routing/pools-and-backends.md).

## `pools`

| Property | Value |
|----------|--------|
| **Type** | List of pool objects |
| **Required** | Yes for a runnable installation (forwarding needs at least one pool with backends) |
| **Location** | Top-level key in the [config file](/control-plane/config-file.md) |

Each list entry is one named pool. Pool `name` values must be **unique** within the file; duplicate names fail [validation](/control-plane/config-file.md).

## Pool object

| Field {: .column-no-wrap } | Type | Required | Default | Description |
|-------|------|----------|---------|-------------|
| `name` | string | yes | — | Identifier referenced by `set_pool`, `retry_pool`, [Rhai](/rhai/index.md) `set_pool` / `set_retry_pool`, [metrics](/observability/metrics.md) labels, and event filters. Must be non-empty and unique among pools. The name `default` is a convention for the catch-all pool when nothing else selects a pool — see [Default pool selection](#default-pool-selection). |
| `backends` | list | yes | — | One or more [backend](#backend-object) entries. An empty list fails validation. |
| `sources_v4` | list of strings | no | (use [forward](/reference/config-schema/index.md) defaults) | IPv4 addresses to bind when forwarding to this pool’s backends. Overrides global `forward.sources_v4` for this pool when non-empty. See [Dual-stack forwarding](/guides/dual-stack-forwarding.md). |
| `sources_v6` | list of strings | no | (use forward defaults) | IPv6 addresses for upstream egress for this pool. Overrides global `forward.sources_v6` when non-empty. |

### Default pool selection

When no [rule](/policy-routing/rules-and-actions.md), [Rhai](/rhai/index.md) script, or [retry](/policy-routing/retries-and-transactions.md) has set a pool for the [transaction](/glossary/index.md#transaction), Conduit picks a pool at Route time:

1. If a pool named **`default`** exists, use it.
2. Otherwise use the **first** pool in the `pools:` list (YAML order).

Naming a catch-all pool `default` is a convention, not a schema requirement — validation does not require that name. If you omit `default`, list order defines the fallback pool. A pool named `default` that appears later in the list still wins over earlier pools when fallback applies.

For examples (split horizon, explicit rules vs catch-all), see [Pools and backends](/policy-routing/pools-and-backends.md).

### Pool `sources_v4` / `sources_v6` constraints

- Each entry must be a valid IPv4 or IPv6 address (not `ip:port`).
- Entries must not be empty strings.
- At most **32** addresses per list (`sources_v4` and `sources_v6` separately).
- When a pool list is empty or omitted, Conduit uses the corresponding global `forward.sources_v4` or `forward.sources_v6` list (if any), then system defaults for bind behavior.

## Backend object

Backends are nested under `pools[].backends`. There is no separate backend `name` field; the upstream is identified by `address` in [metrics](/observability/metrics.md) and logs.

| Field | Type | Required | Default | Description |
|-------|------|----------|---------|-------------|
| `address` | string | yes | — | Upstream resolver as `ip:port`. IPv6 literals use bracket notation, for example `[2001:db8::1]:53`. Must parse as a socket address. |
| `weight` | integer | no | **100** | Load-balancing weight within the pool. If set, must be **≥ 1**. Omitted or unset means effective weight 100. |

## Validation summary

| Rule | Error if violated |
|------|-------------------|
| Unique pool `name` | `duplicate pool name '…'` |
| Non-empty pool `name` | `pool name must not be empty` |
| Pool has ≥ 1 backend | `pool '…' has no backends` |
| Backend `address` parses | `pool '…' backend '…': invalid socket address` |
| Backend `weight` ≥ 1 when set | `pool '…' backend '…' weight must be >= 1` |
| Valid pool `sources_v4` / `sources_v6` | `pool '…': …` (parse/limit messages) |

Validate with `conduitctl validate --file …` or load via the running process; see [Config file](/control-plane/config-file.md).

## Example configuration

```yaml
pools:
  - name: default
    sources_v4:
      - "10.0.0.10"   # Conduit host on the recursor-facing VLAN
    backends:
      - address: "10.0.0.1:53"
        weight: 70
      - address: "10.0.0.2:53"
        # weight omitted → 100
  - name: internal
    sources_v4:
      - "10.0.1.10"   # Conduit host on the internal DNS network
    backends:
      - address: "10.0.1.53:53"
```

## Related topics

- [Pools and backends](/policy-routing/pools-and-backends.md) — behavior and examples
- [Rules and actions](/policy-routing/rules-and-actions.md) — `set_pool` and selectors
- [Retries and transactions](/policy-routing/retries-and-transactions.md) — `retry_pool`
- [Dual-stack forwarding](/guides/dual-stack-forwarding.md) — pool and global egress sources
- [Config schema overview](/reference/config-schema/index.md)
