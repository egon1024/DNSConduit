# Config schema: caches

This page lists the fields for the top-level **`caches:`** list — named DNS answer cache instances referenced by **cache** providers in [Reference: lookup](/reference/config-schema/lookup.md). Backends are **`memory`** (process heap) or **`lmdb`** (on-disk LMDB). For behavior — hit path, negative cache, single-flight, and `on_hit` — see [DNS answer cache](/guides/dns-answer-cache.md).

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
| `type` | string | yes | — | **`memory`** or **`lmdb`** (`ebpf_map` is reserved and rejected) |
| `max_entries` | integer | no | **0** (unlimited) | Cap on live entries; **0** = no limit. Takes effect on the live cache immediately when **apply** or **reload** succeeds (no restart) — see [Reload and apply](#reload-and-apply) |
| `negative_cache` | object | no | see below | NXDOMAIN / NODATA / SERVFAIL caching |
| `on_hit` | object | no | **`response_rules: run`** | Behavior on cache **hit** before [Send](/concepts/architecture-and-packet-path.md#send) |
| `truncated_udp` | object | no | disabled | Opt-in caching of **TC=1** UDP answers |
| `rotate_rrset_on_serve` | boolean | no | **`false`** | Shuffle answer RR order within each RRset on hit |
| `memory` | object | no | see below | Sharding and eviction for **`type: memory`** only |
| `lmdb` | object | yes when `type: lmdb` | — | Path, map size, and capacity pressure for **`type: lmdb`** only |
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

| Field {: .column-no-wrap } | Type | Default | Description |
|-------|------|---------|-------------|
| `shard_count` | integer | **16** | Hash shards for concurrent access; must be **≥ 1** |
| `eviction` | string | **`passive`** | **`passive`** — evict on insert when over `max_entries`; **`active`** — background reaper also trims expired entries |

A **`memory:`** block on **`type: lmdb`** is rejected.

### `lmdb` (`type: lmdb`) { #lmdb }

On-disk LMDB store. Entries survive process restart while still fresh. Expiry is **lazy on read** (no full-database LMDB reaper). Missing environment directories (and parents) are created when the store is opened.

| Field {: .column-no-wrap } | Type | Required | Default | Description |
|-------|------|----------|---------|-------------|
| `path` | string | yes | — | Filesystem path for the LMDB environment directory; created (with any missing parents) when the environment is opened. When the path already exists it must be a readable and writable directory |
| `map_size` | integer or string | yes | — | **Total** LMDB mmap/disk budget across all shard environments under **`path`**, as integer bytes or a decimal **SI** string (`KB` / `MB` / `GB` / `TB` / `PB`); fractional coefficients allowed (for example **`4.5GB`** → 4 500 000 000 bytes). Binary IEC suffixes (`MiB`, `GiB`, …) are **rejected**. Conduit splits the total across shards |
| `shard_count` | integer | no | see below | Number of independent LMDB environments under **`path`** (hash-sharded writers). Must be **≥ 1** and **≤ 64** when set; **`0`** is rejected. When **omitted**: reuse on-disk shard count if a Conduit (or legacy single-env) store already exists at **`path`**; otherwise default to **twice** Lookup concurrency (`sync` → ingress worker count; `split_io` → **`dataplane.policy_workers`**), clamped to **1…64** |
| `when_full` | string | no | **`evict_one`** | Capacity pressure: **`refuse`** \| **`evict_one`** \| **`sample`** — applies when **`max_entries`** binds or the LMDB map is full |
| `sample_size` | integer | no | **16** | Candidate window when **`when_full: sample`**; must be **≥ 1** |
| `sync` | string | no | **`full`** | Commit durability: **`full`** \| **`no_meta`** \| **`none`** — see [Sync durability](#lmdb-sync) |

An **`lmdb:`** block on **`type: memory`** is rejected. Opening an environment with an unsupported on-disk format version **fails** with a message that recommends moving or deleting the environment files — Conduit does not silently migrate or wipe incompatible data.

### Sync durability (`lmdb.sync`) { #lmdb-sync }

Choose how hard Conduit fences LMDB writes to disk. Lead with this decision tree; details follow.

1. Leave **`sync: full`** unless LMDB writes are a bottleneck.
2. Want more write throughput and still care that the environment stays consistent after a crash → **`no_meta`**.
3. Only if still needed, understand filesystem write-ordering risk, and accept wipe-and-refill → **`none`**.

| Value | Rough behavior | Integrity after abrupt host/storage loss |
|-------|----------------|------------------------------------------|
| **`full`** (default) | fsync on commit | Strongest durability for committed fills |
| **`no_meta`** | Data flushed; meta flush deferred | Environment stays consistent; may lose the last commit(s) |
| **`none`** | No flush on commit | Integrity depends on write-order-preserving storage; may lose recent commits or leave a corrupted env if those conditions fail |

Process restart after a clean shutdown is not the same as power loss — dirty pages may still reach disk. A corrupted environment fails open/validate; recover by moving or deleting the files under **`path`**. There is no periodic forced-sync interval knob.

**Dual caps:** **`max_entries`** limits live key count (**`0`** = unlimited) and is enforced as per-shard shares that sum to the configured global cap. **`map_size`** is the **total** mmap/page ceiling split across shards. Space usage and entry gauges aggregate across shards. Multiple shard files under **`path`** are an in-directory writer-parallelism detail — not multi-volume placement.

**Explicit `shard_count` change:** when an explicit clamped value differs from the on-disk layout **N**, apply/reload **Warm-reopens** a new empty shard set (same class as **`lmdb.path`** change): no key migration; abandoned prior files under that **`path`** are removed only after the new set is serving. Failed open **rejects** the apply and keeps the prior store. Omitting **`shard_count`** when a store already exists does **not** abandon solely because the fresh-path 2× heuristic would differ.

## Example

Memory backend:

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

LMDB backend:

```yaml
caches:
  - name: durable
    type: lmdb
    max_entries: 500000
    negative_cache:
      enabled: true
      nxdomain_covers_descendants: true
      servfail_ttl_secs: 10
    lmdb:
      path: /var/lib/conduit/cache/durable
      map_size: 4GB
      when_full: evict_one
```

## Reload and apply { #reload-and-apply }

| Change | Stored in new snapshot? | Live runtime without restart? | Notes |
|--------|-------------------------|-------------------------------|-------|
| `max_entries` on an existing instance | Yes | **Yes** | Cap updates in place immediately when **apply** or **reload** succeeds (no restart); lowering the cap **evicts** entries until at or under the new limit |
| `lmdb.when_full`, `lmdb.sample_size`, `lmdb.sync` | Yes | **Yes** | Hot-applied on the live LMDB backend (same path); sync changes update environment flags without reopen. Failed sync flag update **rejects** the apply |
| `lmdb.map_size` **increase** | Yes | **Yes** | Grows the map in place when LMDB allows; failure **rejects** the apply and keeps the prior map size |
| `lmdb.map_size` **decrease** | Yes | **Yes** (live ladder) | Lookups for that cache **Bypass** (forward) while shrinking; Conduit tries in-place shrink, then evicts until under the new ceiling, then clears all entries if needed. Step that clears entries **discards** cached answers. Failure **rejects** the apply |
| Other cache policy (`negative_cache`, `on_hit`, `truncated_udp`, `rotate_rrset_on_serve`, `memory.eviction`) | Yes | No | Requires process **restart** today |
| `memory.shard_count` | Yes | No | Conduit logs **`pending (restart required)`**; snapshot updates but shard layout is unchanged until restart |
| New cache instance name | Yes | Yes (empty backend) | Reconcile opens a new memory or LMDB backend; open failure **rejects** the apply |
| `lmdb.path` change | Yes | **Yes** (warm reopen) | Opens the new environment first, then atomically switches the live handle; **no** automatic entry migration. Open failure **rejects** the apply; the previous path keeps serving |
| `lmdb.shard_count` explicit change (clamped value ≠ on-disk **N**) | Yes | **Yes** (warm reopen) | Same class as path reopen: opens a new empty shard layout, swaps the live handle, then removes abandoned prior files under **`path`**. **No** key migration. Open failure **rejects** the apply; prior layout keeps serving. Omitting **`shard_count`** keeps on-disk **N** |
| `type` change (`memory` ↔ `lmdb`) | Yes | **Yes** (rebuild) | Tears down the old backend and builds a new empty store on the same instance (single-flight retained); open failure **rejects** the apply |
| Removing an instance from **`caches:`** | Yes | Yes | Drops the runtime instance and closes the LMDB environment (frees mmap and handles); **does not delete** files under **`path`** |
| Memory entries | — | — | **Preserved** across reload/apply on the same instance; **not** preserved across process restart |
| LMDB entries | — | — | **Survive** process restart while still fresh (lazy expiry on read); same-path reload/apply keeps the open environment when layout **N** is unchanged; **`path`** or explicit **`shard_count`** change does **not** copy entries |
| Removing a cache provider from the profile | Yes | Yes | Hot path skips cache when no cache provider is active |

Use **`conduitctl apply`**, **`conduitctl reload`**, or **SIGHUP** — same snapshot swap path as other config. In-flight transactions keep the snapshot they started under.

## Validation summary

| Rule | Error if violated |
|------|-------------------|
| Duplicate `name` | Duplicate cache instance name |
| `type` not **`memory`** or **`lmdb`** | Unsupported cache backend type |
| **`memory:`** on **`type: lmdb`** or **`lmdb:`** on **`type: memory`** | Foreign backend block |
| **`type: lmdb`** without **`lmdb.path`** / **`lmdb.map_size`**, or IEC **`map_size`** suffix | Invalid LMDB config |
| LMDB path exists but is not a directory, or is not readable/writable; create/open failure | Path preflight or open failed |
| Unknown cache reference from lookup provider | Undefined cache name |
| `on_hit.response_rules` not **`run`** or **`skip`** | Invalid on-hit mode |
| `truncated_udp.enabled: true` without `ttl_secs > 0` | Missing or invalid truncated UDP TTL |
| `memory.shard_count` **< 1** | Invalid shard count |
| `memory.eviction` not **`passive`** or **`active`** | Invalid eviction mode |
| `lmdb.when_full` not **`refuse`** / **`evict_one`** / **`sample`** | Invalid when_full |
| `lmdb.sample_size` **< 1** when used with **`sample`** | Invalid sample_size |
| `lmdb.sync` not **`full`** / **`no_meta`** / **`none`** | Invalid sync |
| `lmdb.shard_count` **`0`** | Invalid shard_count (values above **64** are clamped to **64**) |

Unreferenced cache instances (defined in **`caches:`** but not used by any lookup provider) **validate successfully** — for example an instance used only by some lookup profiles, or held for later open-by-name use. Cache attachment for answer lookup/fill is via **lookup profile providers** only (not pool or backend membership).

## Related topics

- [Reference: lookup](/reference/config-schema/lookup.md) — profiles and providers
- [DNS answer cache](/guides/dns-answer-cache.md) — operator guide
- [Built-in metrics — Lookup and cache](/observability/built-in-metrics.md#lookup-and-cache)
