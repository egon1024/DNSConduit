# Metrics configurability

How to choose which built-in metrics Conduit records and exports: bases, categories, collect vs emit, label granularity, event-export counters, and live overlay apply (including Prometheus rebind).

For scrape and OTLP setup, see [Metrics](/observability/metrics.md). For every series name and when it increments, see [Built-in metrics](/observability/built-in-metrics.md). For preset membership and dimension vocabularies, see [Built-in metric registry](/observability/built-in-metric-registry.md).

## Enabling and bases

When the **`metrics:`** section is omitted, built-ins are off. With **`enabled: true`**, choose a **base** that expands into a set of categories:

| Base | Role |
|------|------|
| **`standard`** | Default when enabled and `base` / legacy `profile` are unset. Curated production set — **not** every family in the registry. Opt-in-only families stay off unless you include them. |
| **`minimal`** | Lower cardinality: volume, failures, lookup, health, topology, meta (coarse labels by default). |
| **`none`** | Empty category set — you must **`categories.include`** at least one category, or validation fails. |

```yaml
metrics:
  enabled: true
  base: standard          # none | minimal | standard
  prometheus:
    listen_address: "127.0.0.1:9090"
    path: /metrics
```

Legacy **`metrics.profile`** (`minimal`, `full`, `off`) still loads as an alias; prefer **`base`**. See [Unreleased](/release-notes/unreleased.md) for migration.

## Categories

Resolved set:

```text
C = expand(base) ∪ categories.include − categories.exclude
```

If metrics are enabled and **C** is empty, validation fails. Excluding **`failures`** is allowed but emits a warning (operators often still want failure counters).

| Category | In `minimal` | In `standard` | Typical series |
|----------|--------------|---------------|----------------|
| `volume` | yes | yes | queries, responses, truncations, drops, ACL, queries-by-pool |
| `failures` | yes | yes | parse / forward / script errors, retries, slot exhaustion |
| `lookup` | yes | yes | lookup outcomes, cache lookups |
| `timing` | no | yes | phase / forward / lookup / cache / response duration; forward attempts |
| `cache_detail` | no | yes | fills, singleflight, cache entries / evictions |
| `forward_detail` | no | yes | forward outstanding (scrape) |
| `health` | **yes** | yes | probe results, backend health gauges |
| `runtime` | no | yes | slot in-use / capacity gauges |
| `topology` | yes | yes | pool backends configured, listener / backend info |
| `process` | no | yes | RSS / open fds (Linux scrape) |
| `meta` | yes | yes | build info, start time, config generation |

```yaml
metrics:
  enabled: true
  base: standard
  categories:
    exclude: [timing]       # drop timing from standard
    # include: [process]    # with base: none, include is required
```

## Collect vs emit

Per category (and per Rhai user metric), Conduit separates **collect** (record into the process metric store) from **emit** (include in Prometheus scrape / OTLP push).

| collect | emit | Effect |
|---------|------|--------|
| true | true | Record and export (default when a category is on) |
| true | false | Record only — **still pays hot-path and memory cost**; scrape/OTLP omit the series |
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

**Honest cost:** `collect: true, emit: false` does **not** remove hot-path work. Use it to keep internal counters while hiding series from exporters, not as a free performance switch.

Rhai **`user_metrics[]`** accepts the same **`collect` / `emit`** keys. Legacy **`export: minimal | full`** remains an alias. Scripts that call `metrics.inc` / `metrics.inc_labels` for a metric **must** keep that metric collecting — validate and apply reject stopping collect while a script still references it.

## Granularity

Label dimensions for metric families come from a **default preset** plus optional **per-family overrides** (full replacement of that family's dimension list).

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

Changing a family's dimension list creates a **new series identity** (counters reset for that schema). Overlapping identities keep cumulative values across plan swaps.

Closed dimension vocabularies and membership: [Built-in metric registry](/observability/built-in-metric-registry.md).

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

Defaults are both true when metrics are enabled. **`emit: false`** omits `conduit_events_*` from scrape/OTLP while EventHub may still count internally when collect is true. See [Event export](/observability/event-export.md).

## Overlay and live apply

**`metrics`** may appear in overlay patches. Merge is **deep** (nested maps), not wholesale section replace — details: [Overlay merge strategy](/control-plane/overlay-merge-strategy.md).

| Change | Behavior |
|--------|----------|
| Base, categories, collection, granularity, user_metrics, event_export | Hot on snapshot apply — no process restart; scrape socket stays open if listen settings unchanged |
| Prometheus `listen_address` / `path` | Hot **rebind**: bind new → close old; bind failure **rejects** apply and keeps last-good listener |
| OTLP `endpoint` / TLS | Hot **reconnect**; interval/headers/attrs update in place when only those change |

Rules and Rhai scripts remain file-layer only. Stopping collect for a user metric still referenced by a script fails validate/apply with the script path in the error.

## Related topics

- [Metrics](/observability/metrics.md) — enable export (Prometheus / OTLP)
- [Built-in metric registry](/observability/built-in-metric-registry.md) — category membership and dimensions
- [Built-in metrics](/observability/built-in-metrics.md) — series reference and PromQL
- [Overlay merge strategy](/control-plane/overlay-merge-strategy.md)
- [Operator metrics bases](/guides/operator-metrics-profiles.md) — lab walkthrough
- [Config schema: metrics and tracing](/reference/config-schema/metrics-and-tracing.md)
