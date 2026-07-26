# Metrics configurability

With metrics enabled, Conduit chooses which built-in families to record, how fine-grained their labels are, and applies that selection live — including when the Prometheus listen address changes.

For scrape and OTLP setup, see [Metrics](/observability/metrics.md). For every series name and when it increments, see [Built-in metrics](/observability/built-in-metrics.md). For preset membership and dimension vocabularies, see [Built-in metric registry](/observability/built-in-metric-registry.md).

## Enabling and bases

When the **`metrics:`** section is omitted, no built-in metrics are collected or emitted. With **`enabled: true`**, pick a **base**: a curated starting set of [categories](#categories) which can be considered a starting point for the metrics that are collected/emitted. Use **`categories.include`** and **`categories.exclude`** to add or remove categories from that set — details and the membership table are in the next section.

| Base | Starting set |
|------|--------------|
| **`standard`** | Default when enabled and `base` / legacy `profile` are unset. Production-oriented selection — **not** every category in the registry. |
| **`minimal`** | Lower cardinality: volume, failures, lookup, health, topology, meta (coarse labels by default). |
| **`none`** | Empty set — you must **`categories.include`** at least one category, or validation fails. |

```yaml
metrics:
  enabled: true
  base: standard          # none | minimal | standard
  prometheus:
    listen_address: "127.0.0.1:9090"
    path: /metrics
```

### Legacy aliases { #legacy-profile-alias }

Prefer **`base`** and per-metric **`collect` / `emit`**. These older keys still load through the **1.x** line (with a deprecation warning):

| Former | Equivalent |
|--------|------------|
| `profile: minimal` | `base: minimal` |
| `profile: full` | `base: standard` |
| `profile: off` | `enabled: false` |
| `user_metrics[].export: minimal` \| `full` | `collect` / `emit` (see [Collect vs emit](#collect-vs-emit)) |

Configs that keep using `profile` (and leave the new keys unset) retain the same series identity as before. Configuration fields reference: [Config schema: metrics and tracing](/reference/config-schema/metrics-and-tracing.md).

## Categories

Each base expands to a set of categories. The active set is that expansion, plus **`categories.include`**, minus **`categories.exclude`**.

If metrics are enabled and the result is empty, validation fails. Excluding **`failures`** is allowed but Conduit warns — operators usually still want failure counters. Listing the same category in both **`include`** and **`exclude`** is not an error: Conduit warns and **`exclude`** wins (the category is not active).

| Category {: .column-no-wrap } | In `minimal` | In `standard` | Typical series |
|----------|--------------|---------------|----------------|
| `volume` | <span class="membership-yes">yes</span> | <span class="membership-yes">yes</span> | queries, responses, truncations, drops, ACL, queries-by-pool |
| `failures` | <span class="membership-yes">yes</span> | <span class="membership-yes">yes</span> | parse / forward / script errors, retries, slot exhaustion |
| `lookup` | <span class="membership-yes">yes</span> | <span class="membership-yes">yes</span> | lookup outcomes, cache lookups |
| `timing` | <span class="membership-no">no</span> | <span class="membership-yes">yes</span> | phase / forward / lookup / cache / response duration; forward attempts |
| `cache_detail` | <span class="membership-no">no</span> | <span class="membership-yes">yes</span> | fills, singleflight, cache entries / evictions |
| `forward_detail` | <span class="membership-no">no</span> | <span class="membership-yes">yes</span> | forward outstanding (scrape) |
| `health` | <span class="membership-yes">yes</span> | <span class="membership-yes">yes</span> | probe results, backend health gauges |
| `runtime` | <span class="membership-no">no</span> | <span class="membership-yes">yes</span> | slot in-use / capacity gauges |
| `topology` | <span class="membership-yes">yes</span> | <span class="membership-yes">yes</span> | pool backends configured, listener / backend info |
| `process` | <span class="membership-no">no</span> | <span class="membership-yes">yes</span> | RSS, FDs, threads, CPU (Linux scrape) |
| `meta` | <span class="membership-yes">yes</span> | <span class="membership-yes">yes</span> | build info, start time, config generation |

```yaml
metrics:
  enabled: true
  base: standard
  categories:
    exclude: [timing]       # drop timing from standard
    # include: [process]    # with base: none, include is required
```

## Collect vs emit

For each category (and each [Rhai user metric](/rhai/user-metrics.md)), Conduit separates **collect** from **emit**:

- **collect** — record into the process metric store (pays hot-path and memory cost when true)
- **emit** — include the series in Prometheus scrape and OTLP push

| collect | emit | Effect |
|---------|------|--------|
| true | true | Record and export (default when a category is on) |
| true | false | Record only — the series stay out of Prometheus scrape and OTLP push; **hot-path and memory cost remain**. Little practical use today; a later release will let Rhai scripts read collected values even when they are not emitted. |
| false | false | Neither record nor export |
| false | true | **Invalid** — rejected at validate |

```yaml
metrics:
  enabled: true
  base: standard
  collection:
    timing:
      collect: true
      emit: false
```

Turning emit off does not remove hot-path work — use `collect: false` (or drop the category) when you want to stop paying that cost.

Rhai **`user_metrics[]`** accepts the same **`collect` / `emit`** keys, plus optional **`help`** (Prometheus `# HELP` / OTel description). Legacy **`export: minimal | full`** remains an alias.

**Defaults for script metrics not listed under `user_metrics`:**

| Base | Unlisted script metrics |
|------|-------------------------|
| **`standard`** | Collect and emit **on** — no list required |
| **`minimal`** | Collect and emit **off** — scripts may still call `metrics.inc` / `metrics.inc_labels`; increments **no-op** (same as a built-in category with collect off). Validate and apply **succeed** and emit a **warning** listing the script path and line |

When collect or emit is off for a metric that scripts still write, Conduit logs a warning. Future **read** APIs that need live values will reject collect-off while they still reference the metric.

```yaml
metrics:
  enabled: true
  base: minimal
  user_metrics:
    - name: block_hits
      help: Policy block hits by category
      collect: true
      emit: true
```

On **`base: minimal`**, list metrics under **`user_metrics`** with collect (and usually emit) on when you want them recorded and scraped. On **`base: standard`**, use **`user_metrics`** only when you need **`help`** or a different collect/emit pair.

## Granularity

Label dimensions for metric families come from a **default preset**, which you can override per family (a full replacement of that family's dimension list).

| Default when | Preset |
|--------------|--------|
| `base: minimal` | `coarse` |
| `base: standard` | `fine` (same label schemas as the former `profile: full` when you set no overrides) |

```yaml
metrics:
  enabled: true
  base: standard
  granularity:
    default: fine                 # coarse | balanced | fine
    timing: [pool]                # full replace for timing families
    responses:
      rcode: coarse               # coarse class buckets vs IANA names
```

Changing a family's dimension list while Conduit is running creates a **new series identity** — counters for that schema start over. Families whose identity is unchanged keep their cumulative values across plan swaps.

For which label keys each family allows, and which families each base includes, see [Built-in metric registry](/observability/built-in-metric-registry.md).

## Event export counters

`conduit_events_*` sink counters are controlled separately from dataplane categories:

```yaml
metrics:
  enabled: true
  base: standard
  event_export:
    collect: true
    emit: true
```

When metrics are enabled, both default to true. **`emit: false`** omits `conduit_events_*` from scrape/OTLP; EventHub may still count internally when collect is true. See [Event export](/observability/event-export.md).

## Overlay and live apply

You can change **`metrics`** through [overlay](/glossary/index.md#overlay) patches. Merge is **deep** (nested maps), not a wholesale section replace — details: [Overlay merge strategy](/control-plane/overlay-merge-strategy.md).

| Change | Behavior |
|--------|----------|
| Base, categories, collection, granularity, user_metrics, event_export | Apply on the next successful config apply — no process restart; the scrape socket stays open if listen settings are unchanged |
| Prometheus `listen_address` / `path` | Live **rebind**: bind the new address, then close the old one. If bind fails, apply is **rejected** and the previous listener keeps serving |
| OTLP `endpoint` / TLS | Live **reconnect**; interval, headers, and attributes update in place when only those change |

Stopping collect (or emit) for a user metric that scripts still **write** produces a **warning** (increments no-op / series stay out of export); validate and apply still succeed. Future **read** APIs will reject collect-off while they still reference the metric.

## Related topics

- [Metrics](/observability/metrics.md) — enable export (Prometheus / OTLP)
- [User metrics](/rhai/user-metrics.md) — Rhai `conduit_user_*` collect/emit and collect-off warnings
- [Built-in metric registry](/observability/built-in-metric-registry.md) — category membership and dimensions
- [Built-in metrics](/observability/built-in-metrics.md) — series reference and PromQL
- [Overlay merge strategy](/control-plane/overlay-merge-strategy.md)
- [Operator metrics bases](/guides/operator-metrics-bases.md) — **`minimal`** vs **`standard`** lab
- [Metrics beyond bases](/guides/metrics-beyond-bases.md) — categories, collect/emit, granularity, overlay rebind
- [Config schema: metrics and tracing](/reference/config-schema/metrics-and-tracing.md)
