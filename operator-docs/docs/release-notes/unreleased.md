# Unreleased

## LMDB DNS answer cache

You can now keep DNS answers on disk so they survive a Conduit restart — not only in memory.

- **On-disk cache:** Set a cache instance to **`type: lmdb`**. Answers stay available across restart while their TTL is still valid. In-memory caches (`type: memory`) work as before. See [DNS answer cache](/guides/dns-answer-cache.md) and [Reference: caches](/reference/config-schema/caches.md).
- **What you must set:** **`lmdb.path`** (directory for the store — Conduit creates it if needed) and **`lmdb.map_size`** (how large the store may grow, as bytes or sizes like **`64MB`** / **`4GB`** — not **`MiB`** / **`GiB`**). A ready-to-run example is packaged under `/usr/share/doc/conduit/examples/dns-answer-cache/`.
- **Faster concurrent writes (optional):** **`lmdb.shard_count`** splits the store into several files under **`path`** so writers do not all contend on one file (1…64). Leave it unset to keep an existing layout, or to get a sensible default on a new path. Changing an explicit shard count that does not match what is already on disk opens a **new empty** store (previous entries are not copied over). If that open fails, Conduit keeps the old store and rejects the config change.
- **When the cache is full:** **`lmdb.when_full`** chooses what happens at the entry or size limit — drop one entry (**`evict_one`**, default), refuse new fills (**`refuse`**), or pick among a small sample (**`sample`**, with **`sample_size`**).
- **How hard to flush to disk:** **`lmdb.sync`** controls durability vs write cost — **`full`** (default, safest), **`no_meta`**, **`periodic`** (flush on a timer via **`lmdb.sync_interval`**, default **1s**), or **`none`**. Prefer the [sync decision tree](/reference/config-schema/caches.md#lmdb-sync) over guessing from throughput alone. You can change **`sync`** and **`sync_interval`** with **apply** / **reload** without restarting Conduit.
- **Most LMDB knobs apply live:** Size limits, full-cache behavior, sync settings, growing or shrinking **`map_size`**, and changing **`path`** take effect after a successful **apply** or **reload** — no process restart. If a change cannot be applied (for example the new path will not open), Conduit keeps the previous store and rejects the update. Switching an instance between **`memory`** and **`lmdb`** starts an empty cache under that name. Details: [Reload and apply](/reference/config-schema/caches.md#reload-and-apply).
- **Metrics:** Watch entry and byte usage, shard count, sync mode, full-cache refusals, and (for **`periodic`**) how long ago the last flush ran and whether it failed. The same series appear in Prometheus and OTLP. See [Built-in metrics — Lookup and cache](/observability/built-in-metrics.md#lookup-and-cache).
- **Checked in the interop matrix:** [`cache-lmdb-durability-restart-hit`](/interop/cases/cache-lmdb-durability-restart-hit.md) — after restart, a cached answer is served without asking the upstream again.

### Getting started

Configs that already use **`type: memory`** need no edits. To keep answers across restart: use **`type: lmdb`** with **`path`** and **`map_size`**, put a **cache** step before **forward** in the lookup profile, then **`conduitctl validate`** and **apply** / **reload**. If files at **`path`** are from an unsupported older layout, Conduit refuses to open them and tells you to move or delete them — it will not delete them for you.

## Performance notes (LMDB)

- **Warm cache (mostly hits):** How memory and LMDB compare after the cache is already filled. See [Memory vs LMDB warm cache_hit](/performance/studies/memory-vs-lmdb-cache-hit.md).
- **Busy cache (lots of fill and eviction):** How memory and each LMDB sync mode compare when the cache is under constant write pressure. Use those figures for relative guidance — not as capacity guarantees. See [Memory vs LMDB high-churn cache](/performance/studies/memory-vs-lmdb-cache-churn.md) and [Performance findings](/performance/index.md#findings).

## Other changes

- **Cache name in debug logs and traces:** Debug **`query complete`** lines include **`cache=`** (the cache name on a hit, or **`-`** otherwise). **`conduitctl trace`** shows the same on cache steps. See [Logging](/observability/logging.md) and [Tracing](/observability/tracing.md).
- **Clearer UDP listener check:** If a UDP listener uses more than one thread, config validation now requires **`reuse_port: true`** up front, instead of failing later when binding the socket. See [Reference: listeners](/reference/config-schema/listeners.md).
