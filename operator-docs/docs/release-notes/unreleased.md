# Unreleased

Changes merged to `main` that are not yet tagged. Staging area for the next operator release.

## Breaking changes — Lookup spine { #breaking-changes--lookup-spine }

Answer production now runs through a single **Lookup** pipeline phase instead of separate top-level **Route**, **Forward**, and **Wait for response** steps. Pool selection, upstream I/O, and suspend/resume still behave the same; they run inside the **forward** lookup provider.

### What changes for operators

| Area | Before | After |
|------|--------|-------|
| Sparse configs (no `lookup:` block) | Route → Forward → Wait for response | Lookup with an implicit forward-only profile — **same forwarding behavior** |
| Traces (`full` profile) | Top-level `route`, `forward`, `wait_response` phase events | Top-level **`lookup`** only; routing and upstream wait appear as nested events inside Lookup |
| Built-in metrics — [`conduit_phase_duration_seconds`](/observability/built-in-metrics.md#conduit_phase_duration_seconds) (`full` profile) | `phase` label values `route`, `forward`, `wait_response` | `phase` label **`lookup`** for answer-production time (no separate top-level route/forward/wait series) |
| Retry from [Response rules](/concepts/architecture-and-packet-path.md#response-rules) | Re-entered at Route | Re-enters **Lookup** (full provider chain, subject to cache eligibility) |

### Operator action

1. If you rely on trace phase names or PromQL on `conduit_phase_duration_seconds{phase="forward"}`, update dashboards and alerts to use **`lookup`** (and nested trace messages where needed).
2. Re-run **`conduitctl validate --file <config>`** after upgrade; existing sparse YAML without `lookup:` should validate unchanged.
3. See [Architecture and packet path](/concepts/architecture-and-packet-path.md) (updated in this release) for the Lookup-centric packet path.

---

## New features — DNS answer cache { #new-features--dns-answer-cache }

Optional in-memory DNS answer caching is available when you add a **`caches:`** catalog and a **cache** provider before **forward** in **`lookup.profiles`**.

### Configuration surface

- **`caches[]`** — named cache instances (`type: memory`)
- **`lookup.profiles.<name>.providers`** — ordered provider list (`cache` then `forward` is typical)
- Per-instance policy on each cache:
  - **`negative_cache`** — NXDOMAIN/NODATA; **`nxdomain_covers_descendants`** (default **true**); **`servfail_ttl_secs`** (default **10**; **0** = do not cache SERVFAIL)
  - **`truncated_udp`** — opt-in TC=1 UDP caching (`enabled` + required **`ttl_secs`** when enabled)
  - **`on_hit.response_rules`** — whether [Response rules](/concepts/architecture-and-packet-path.md#response-rules) run after a **cache hit** (`run` or `skip`; see below)
  - **`rotate_rrset_on_serve`** — on cache hits, shuffle answer RR order within each RRset before Send (default **false**; see below)
  - **`memory.shard_count`**, **`memory.eviction`** (`passive` default, `active` opt-in)
  - **`max_entries`**

Example (cache then forward on profile `default`):

```yaml
caches:
  - name: global
    type: memory
    max_entries: 100000
    negative_cache:
      enabled: true
      nxdomain_covers_descendants: true
      servfail_ttl_secs: 10
lookup:
  profiles:
    default:
      providers:
        - type: cache
          cache: global
        - type: forward
```

Configs that **omit** `lookup:` continue to use an implicit **`default`** profile with a single **forward** provider — no cache allocation on the hot path.

### Cache behavior highlights

- **Cache hit** — serves stored wire from memory; **no upstream forward attempt** for that transaction attempt.
- **Cache miss** — consults the next provider (typically forward).
- **Parallel identical queries** — single upstream fetch via **single-flight**; waiters resume when the fill completes.
- **Fill** — stores the upstream answer wire **before** [Response rules](/concepts/architecture-and-packet-path.md#response-rules) mutate it; authority and additional sections are preserved.
- **TTL on serve** — answer TTLs decay by elapsed time since fill; stored slab bytes are not mutated.
- **In-memory only** — cache entries are lost on process restart.
- **Reload / apply** — `lookup` and `caches` policy updates take effect for **new queries** via the normal snapshot swap; in-flight transactions keep the snapshot they started under.

### `on_hit.response_rules` — `run` vs `skip`

When the cache serves a hit, Conduit already has a complete answer wire. **`on_hit.response_rules`** controls whether the pipeline still runs [Response rules](/concepts/architecture-and-packet-path.md#response-rules) on that hit — the same built-in response rules and response-hook Rhai that run after an upstream forward — or sends the cached answer straight to the client.

| Value | Cache hit path |
|-------|----------------|
| **`run`** (default when `on_hit` is omitted) | Run Response rules on the cached answer, then [Send](/concepts/architecture-and-packet-path.md#send) — same policy hooks as a forward-produced answer |
| **`skip`** | Skip Response rules; go straight to Send (lower latency when response rules only matter on cache misses) |

If you use response-hook **`metrics.inc`** only for upstream outcomes, cache hits will **not** increment those counters when **`skip`** is set. Use default **`run`**, built-in metrics (below), or record metrics on the request hook.

### `rotate_rrset_on_serve`

When **true**, each cache **hit** returns the same RRs as stored but may **reorder** records within each answer RRset (for example multiple **A** or **AAAA** records for one name) using a random cyclic offset per RRset. That spreads client load across peers when upstream returned several addresses in a fixed order. Default **false** — served order matches the stored wire aside from query ID rewrite and TTL decay; the cache slab is never mutated.

---

## New features — Observability and Rhai { #new-features--observability-and-rhai }

### Built-in metrics

New and extended series (Prometheus scrape and OTLP push with equivalent semantics):

| Series | Profile | Purpose |
|--------|---------|---------|
| [`conduit_lookup_provider_outcomes_total`](/observability/built-in-metrics.md) | `minimal` + `full` | Terminal lookup provider outcomes (`profile`, `provider`, `outcome`) |
| [`conduit_cache_lookups_total`](/observability/built-in-metrics.md) | `minimal` + `full` | Cache read path (`cache`, `profile`, `result`: hit / miss / bypass) |
| [`conduit_responses_total`](/observability/built-in-metrics.md#conduit_responses_total) | `minimal` + `full` | New label **`answer_source`**: `cache` or `forward` |
| [`conduit_responses_truncated_total`](/observability/built-in-metrics.md#conduit_responses_truncated_total) | `minimal` + `full` | Same **`answer_source`** label |
| `conduit_cache_fills_total`, `conduit_cache_singleflight_coalesced_total` | `full` only | Stores and single-flight waiters resolved |
| `conduit_lookup_duration_seconds`, `conduit_cache_lookup_duration_seconds`, `conduit_response_duration_seconds` | `full` only | Latency split by provider / cache / answer source |

Forward series ([`conduit_forward_attempts_total`](/observability/built-in-metrics.md#conduit_forward_attempts_total), [`conduit_forward_duration_seconds`](/observability/built-in-metrics.md#conduit_forward_duration_seconds)) increment only when the forward provider **actually attempts** upstream I/O — not on cache hit short-circuit.

Example PromQL (cache vs forward response volume):

```promql
sum(rate(conduit_responses_total[5m])) by (listener, answer_source)
```

### Tracing and event export

- Lookup phase entry and per-provider nested events; cache hit without forward route events.
- Event-sink and built-in rule selectors **`answer_source`** (`cache` | `forward`) and **`cache_instance`** (exact cache name).
- Dnstap **`extra_fields`** may include **`answer_source`** and **`cache_instance`**.

### Rhai (response and request hooks)

| API | Hook | Notes |
|-----|------|-------|
| **`txn.set_cache_lookup_eligible(bool)`** | Request | Default **true**; set **false** to bypass cache for this query |
| **`txn.answer_source()`** | Response | Returns **`cache`**, **`forward`**, or empty string before an answer exists |
| **`txn.cache_instance()`** | Response | Cache instance name on cache hits; empty otherwise |

**`txn.last_forward_ms()`** returns **0** when no forward attempt ran (for example a cache hit). It measures upstream forward RTT only — use **`txn.answer_source()`** or built-in metrics to distinguish cache from forward.

**`lookup(table, key)`** is unchanged — it reads **`data_sources`** policy tables, not the Lookup pipeline phase. See [Data sources and lookups](/rhai/data-sources-and-lookups.md).

---

## Upgrade notes

- **Sparse / minimal configs** — omitting `lookup:` keeps forward-only behavior; no YAML change required unless you want caching.
- **Enable caching** — add `caches:` and a cache provider entry before `forward` in `lookup.profiles.default` (or your active profile).
- **Custom response-hook metrics** — review cache-hit behavior with default **`on_hit.response_rules: run`**; set **`skip`** only if response rules are intentionally miss-only.
- **Dashboards** — update trace and `conduit_phase_duration_seconds` queries that assumed top-level `route` / `forward` / `wait_response` phases.
- **Validation** — invalid cache references, invalid `on_hit.response_rules`, and enabled `truncated_udp` without `ttl_secs` fail at **`conduitctl validate`** time; failed reload/apply retains the last-good snapshot.

### Documentation (this release)

Canonical operator pages updated for Lookup, cache configuration, built-in metrics, and Rhai txn APIs — see [Architecture and packet path](/concepts/architecture-and-packet-path.md), [Built-in metrics](/observability/built-in-metrics.md), and [Transaction API](/rhai/txn-api.md).

---

_Draft — review before tagging. Links and section anchors may change as operator-docs are finalized._
