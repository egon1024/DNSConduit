# Config schema: caches

This page lists the fields for the top-level **`caches:`** list — named in-memory DNS answer cache instances referenced by **cache** providers in [Reference: lookup](/reference/config-schema/lookup.md). For behavior — hit path, negative cache, single-flight, and `on_hit` — see [DNS answer cache](/guides/dns-answer-cache.md).

## `caches`

| Property | Value |
|----------|--------|
| **Type** | List of cache instance objects |
| **Required** | No — omit when no cache provider is configured |
| **Location** | Top-level key in the [config file](/control-plane/config-file.md) |

Each entry **`name`** must be unique. Lookup cache providers reference instances by name.

## Instance fields

| Field {: .column-no-wrap } | Type | Required | Default | Description |
|-------|------|----------|---------|-------------|
| `name` | string | yes | — | Instance name used in `lookup.profiles.*.providers[].cache` |
| `type` | string | yes | — | **`memory`** only in this release |
| `max_entries` | integer | no | **0** (unlimited) | Cap on live entries; **0** = no limit. Takes effect on the live cache immediately when **apply** or **reload** succeeds (no restart) — see [Reload and apply](#reload-and-apply) |
| `negative_cache` | object | no | see below | NXDOMAIN / NODATA / SERVFAIL caching |
| `on_hit` | object | no | **`response_rules: run`** | Behavior on cache **hit** before [Send](/concepts/architecture-and-packet-path.md#send) |
| `truncated_udp` | object | no | disabled | Opt-in caching of **TC=1** UDP answers |
| `rotate_rrset_on_serve` | boolean | no | **`false`** | Shuffle answer RR order within each RRset on hit |
| `memory` | object | no | see below | Sharding and eviction for `type: memory` |
| `key` | object | no | — | Reserved for future key augmenters — not configurable today |

### `negative_cache`

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `enabled` | boolean | **`true`** | When **`false`**, positive answers may still cache; negative answers are not stored |
| `nxdomain_covers_descendants` | boolean | **`true`** | When **`true`**, a cached NXDOMAIN for a name also satisfies descendant queries |
| `servfail_ttl_secs` | integer | **10** | TTL for cached SERVFAIL; **0** = do not cache SERVFAIL |

When **`negative_cache`** is omitted, negative caching is **enabled** with the defaults above.

### `on_hit`

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `response_rules` | string | **`run`** | **`run`** — run [Response rules](/concepts/architecture-and-packet-path.md#response-rules) on cache hits; **`skip`** — go straight to [Send](/concepts/architecture-and-packet-path.md#send) |

See [DNS answer cache — on_hit tradeoff](/guides/dns-answer-cache.md#on_hit-response_rules) for impact on response-hook Rhai and **`metrics.inc`**.

### `truncated_udp`

Opt-in storage of truncated UDP upstream answers (TC bit set). Keys are distinct from **complete** answers. Truncated stubs are served only to **UDP** clients; TCP clients miss and continue the lookup chain. When a **complete** answer is later stored for the same query dimensions, any truncated sibling is removed — see [DNS answer cache — Cache key dimensions](/guides/dns-answer-cache.md#cache-key-dimensions).

| Field | Type | Required when | Description |
|-------|------|---------------|-------------|
| `enabled` | boolean | — | Default **`false`** |
| `ttl_secs` | integer | **`enabled: true`** | Required and **> 0** when enabled |

### `memory` (`type: memory`)

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `shard_count` | integer | **16** | Hash shards for concurrent access; must be **≥ 1** |
| `eviction` | string | **`passive`** | **`passive`** — evict on insert when over `max_entries`; **`active`** — background reaper also trims expired entries |

## Example

```yaml
caches:
  - name: global
    type: memory
    max_entries: 100000
    negative_cache:
      enabled: true
      nxdomain_covers_descendants: true
      servfail_ttl_secs: 10
    on_hit:
      response_rules: run
    memory:
      shard_count: 16
      eviction: passive

lookup:
  profiles:
    default:
      providers:
        - type: cache
          cache: global
        - type: forward
```

## Reload and apply { #reload-and-apply }

| Change | Stored in new snapshot? | Live runtime without restart? | Notes |
|--------|-------------------------|-------------------------------|-------|
| `max_entries` on an existing instance | Yes | **Yes** | Cap updates in place immediately when **apply** or **reload** succeeds (no restart); lowering the cap **evicts** entries until at or under the new limit |
| Other cache policy (`negative_cache`, `on_hit`, `truncated_udp`, `rotate_rrset_on_serve`, `memory.eviction`) | Yes | No | Requires process **restart** today |
| `memory.shard_count` | Yes | No | Conduit logs **`pending (restart required)`**; snapshot updates but shard layout is unchanged until restart |
| New cache instance name | Yes | Yes (empty backend) | Reconcile adds a new in-memory backend |
| In-memory entries | — | — | **Preserved** across reload/apply on the same instance; **not** preserved across process restart |
| Removing a cache provider from the profile | Yes | Yes | Hot path skips cache when no cache provider is active |

Use **`conduitctl apply`**, **`conduitctl reload`**, or **SIGHUP** — same snapshot swap path as other config. In-flight transactions keep the snapshot they started under.

## Validation summary

| Rule | Error if violated |
|------|-------------------|
| Duplicate `name` | Duplicate cache instance name |
| `type` not **`memory`** | Unsupported cache backend type |
| Unknown cache reference from lookup provider | Undefined cache name |
| `on_hit.response_rules` not **`run`** or **`skip`** | Invalid on-hit mode |
| `truncated_udp.enabled: true` without `ttl_secs > 0` | Missing or invalid truncated UDP TTL |
| `memory.shard_count` **< 1** | Invalid shard count |
| `memory.eviction` not **`passive`** or **`active`** | Invalid eviction mode |

Unreferenced cache instances (defined in **`caches:`** but not used by any lookup provider) **validate successfully** — for example an instance used only by some lookup profiles, or held for later open-by-name use. Cache attachment for answer lookup/fill is via **lookup profile providers** only (not pool or backend membership).

## Related topics

- [Reference: lookup](/reference/config-schema/lookup.md) — profiles and providers
- [DNS answer cache](/guides/dns-answer-cache.md) — operator guide
- [Built-in metrics — Lookup and cache](/observability/built-in-metrics.md#lookup-and-cache)
