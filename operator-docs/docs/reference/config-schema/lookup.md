# Config schema: lookup

This page lists the fields for the top-level **`lookup:`** block — named **lookup profiles** and ordered **providers** that produce DNS answers after [Request rules](/concepts/architecture-and-packet-path.md#request-rules). For behavior — provider chain, cache hits, and retry re-entry — see [Architecture and packet path — Lookup](/concepts/architecture-and-packet-path.md#lookup) and [DNS answer cache](/guides/dns-answer-cache.md).

Cache instance fields are described in [Reference: caches](/reference/config-schema/caches.md).

## `lookup`

| Property | Value |
|----------|--------|
| **Type** | Mapping (object) |
| **Required** | No — when omitted, Conduit synthesizes profile **`default`** with a single **forward** provider |
| **Location** | Top-level key in the [config file](/control-plane/config-file.md) |

## Block fields

| Field {: .column-no-wrap } | Type | Required | Default | Description |
|-------|------|----------|---------|-------------|
| `profiles` | map of profile name → profile object | no | implicit **`default`** forward-only | Named lookup profiles; each profile lists providers in declaration order |

### Profile object (`lookup.profiles.<name>`)

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `providers` | list of provider objects | yes (when profile is explicit) | Ordered provider chain — Conduit stops on `Answered`, `Fail`, or `Pending` |

### Provider object (`providers[]`)

| Field | Type | Required | When | Description |
|-------|------|----------|------|-------------|
| `type` | string | yes | always | **`cache`** or **`forward`** |
| `cache` | string | yes when `type: cache` | cache provider | Name of a [cache instance](/reference/config-schema/caches.md) from **`caches:`** |

Typical cache-enabled profile:

```yaml
lookup:
  profiles:
    default:
      providers:
        - type: cache
          cache: global
        - type: forward
```

Forward-only (equivalent to omitting `lookup:`):

```yaml
lookup:
  profiles:
    default:
      providers:
        - type: forward
```

## Implicit default profile

When **`lookup:`** is omitted, snapshot compile adds profile **`default`** with one **`forward`** provider. No cache backend is allocated on the hot path. Existing minimal YAML without `lookup:` validates and forwards the same as before the Lookup spine.

## Provider order

Providers run **top to bottom** within the active profile:

| Order | Effect |
|-------|--------|
| **cache** then **forward** | Try cache first; on miss, run forward in the same Lookup pass |
| **cache** only | Cache hits answer; misses proceed to Response rules with no stored answer |
| **forward** only | Same as implicit default — no cache read |

## Retry and Lookup

[Response rules](/concepts/architecture-and-packet-path.md#response-rules) retry sends the pipeline back to **Lookup** (not a separate Route phase). The **full provider chain** runs again. [Cache lookup eligibility](/guides/dns-answer-cache.md#cache-eligibility) is cleared when the forward provider starts upstream I/O, so retries after a forward attempt do not re-read cache unless request policy set eligibility back to true.

## Reload and apply

| Change | Stored in new snapshot? | Notes |
|--------|-------------------------|-------|
| `lookup:` profiles and provider order | Yes | New queries use the updated profile |
| `caches:` — `max_entries` | Yes | Live cap updates immediately on **apply** or **reload** (no restart) — see [Caches — Reload](/reference/config-schema/caches.md#reload-and-apply) |
| `caches:` — LMDB `when_full` / `sample_size` / `sync` / `sync_interval` / `map_size` increase | Yes | Hot-applied on the live LMDB backend (same path); `map_size` decrease is not applied yet |
| Other `caches:` policy | Yes | Restart required for live behavior today (except the hot fields above) |
| In-flight transactions | — | Keep the snapshot they started under |

## Validation summary

| Rule | Error if violated |
|------|-------------------|
| Unknown `cache` name on a cache provider | Reference to undefined cache instance |
| `type` not **`cache`** or **`forward`** | Invalid provider type |
| Cache provider without `cache` field | Missing cache instance name |
| Empty `providers` list | Profile must list at least one provider |

## Related topics

- [Reference: caches](/reference/config-schema/caches.md) — cache instance catalog and policy keys
- [DNS answer cache](/guides/dns-answer-cache.md) — operator guide
- [Architecture and packet path — Lookup](/concepts/architecture-and-packet-path.md#lookup)
- [Built-in metrics — Lookup and cache](/observability/built-in-metrics.md#lookup-and-cache)
