---
toc_depth: 3
---

# User metrics

Rhai scripts publish custom counters through the **`metrics`** scope object: **`metrics.inc`** and **`metrics.inc_labels`**. Exported series use the prefix **`conduit_user_<name>`** (for example `conduit_user_block_hits`).

**`metrics`** is separate from **`txn`** — counters are not per-query policy state. See [Host API overview](/rhai/host-api.md) for how the scopes fit together.

[Collect vs emit](/observability/metrics-configurability.md#collect-vs-emit) on the metrics plan controls whether each user metric is recorded and whether it appears in Prometheus / OTLP. See [Collect and emit](#export-tier) below.

## Declaring metrics in scripts

Metrics are discovered at **snapshot compile** by scanning Rhai source for **`metrics.inc("name", …)`** and **`metrics.inc_labels("name", …)`** calls. The metric name and label keys must be consistent across all scripts in the snapshot.

```rhai
metrics.inc("block_hits", 1);
metrics.inc_labels("block_hits", 1, #{ category: "eu" });
```

| Rule | Behavior |
|------|----------|
| Name | ASCII alphanumeric and `_` in source; exported as `conduit_user_<name>` |
| Labels | Declared in the `#{ key: value, … }` map on first scan; keys must match on every call |
| Disallowed label keys | `qname`, `client`, `client_ip`, `txn_id`, and other high-cardinality keys — see compile errors |
| Unregistered name at runtime | Script error (`metric not registered at script load`) |

Scripts always **write** metrics; they cannot read counter values back. Use [tags](/rhai/txn-api.md#tags), [lookups](/rhai/data-sources-and-lookups.md), or **`txn`** state for per-query policy.

## Collect and emit { #export-tier }

Each user metric has **collect** (record into the process store) and **emit** (include in Prometheus scrape / OTLP push):

| collect | emit | Effect |
|---------|------|--------|
| true | true | Record and export (usual default when the metric is on) |
| true | false | Record only — scrape/OTLP omit the series; still pays hot-path cost |
| false | false | Neither record nor export |
| false | true | **Invalid** — rejected at validate |

**Defaults when a metric is not listed under `user_metrics[]`:**

| Active plan | Default for unlisted script metrics |
|-------------|-------------------------------------|
| **`base: standard`** (fine granularity) | collect + emit **on** |
| **`base: minimal`** (coarse granularity) | collect + emit **off** |

On **`minimal`**, unlisted script metrics default to collect and emit **off**. Scripts may still call `metrics.inc` / `metrics.inc_labels`; increments **no-op** (same as a built-in category with collect off). Validate and apply **succeed** and emit a **warning** that lists the script path and line. List each metric under **`metrics.user_metrics`** with collect (and usually emit) on when you want them recorded and scraped — or use **`base: standard`**.

### Config overrides

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

| Field | Meaning |
|-------|---------|
| `name` | Metric name from `metrics.inc` (without `conduit_user_` prefix) |
| `help` | Optional Prometheus HELP / OTel description; omit for the default **"Rhai user-defined metric"** |
| `collect` / `emit` | Preferred controls (see table above) |
| `export` | **Deprecated** alias: **`minimal`** → collect+emit always on; **`full`** → collect+emit only on a standard-tier plan |

Validation:

- Each `name` must match a metric registered by at least one Rhai script at compile time.
- Duplicate `name` entries are rejected.
- Unknown names fail snapshot build (`conduitctl validate`).
- Collect or emit off while scripts still write the metric is allowed; Conduit warns (script path listed). Future read APIs will reject collect-off while they still reference the metric.

Prefer **`base: standard`** (or an explicit **`user_metrics`** collect override) for labs that scrape `conduit_user_*` series. Details: [Metrics configurability](/observability/metrics-configurability.md).

## Export path

When `metrics.enabled` is true, successful hook runs flush **collecting** user-metric deltas into the process-wide user registry. [Prometheus scrape](/observability/metrics.md) and [OTEL push](/observability/metrics.md) include `conduit_user_*` series that also have **emit** true, alongside built-ins. Optional **`help`** on `user_metrics[]` sets the Prometheus `# HELP` line and the OTel instrument description (same string on both sinks); the metric **name** stays `conduit_user_<name>`.

Recording does not require an export listener — counters accumulate in memory when collect is true. Configure `prometheus` and/or `otel` to observe them externally.

## Examples

| Script | Config intent | Notes |
|--------|---------------|-------|
| `block-hits.rhai` | `base: standard` | `block_hits` with `category` label; default collect+emit |
| `block-hits.rhai` | `base: minimal` + `user_metrics` collect true | `block_hits` opted in on a minimal base |
| `slow-login-alert.rhai` | `base: standard` | `slow_login` when `txn.last_forward_ms() > 500` |

## Cache hits and on_hit skip { #cache-hits-and-on_hit-skip }

Response-hook **`metrics.inc`** runs only when [Response rules](/concepts/architecture-and-packet-path.md#response-rules) run. On a **cache hit** with **`on_hit.response_rules: skip`**, the response hook is **not** invoked — custom counters on that hook will not increment.

Options:

- Keep default **`on_hit.response_rules: run`** so response rules and metrics run on hits
- Use built-in [`conduit_responses_total`](/observability/built-in-metrics.md#conduit_responses_total) with **`answer_source`**
- Record metrics on the **request** hook when classification is enough

See [DNS answer cache — on_hit tradeoff](/guides/dns-answer-cache.md#on_hit-response_rules).

## Related topics

- [Metrics configurability](/observability/metrics-configurability.md) — bases, collect/emit, collect-off warnings
- [Built-in metrics — User metrics](/observability/built-in-metrics.md#user-metrics-rhai)
- [Host API overview](/rhai/host-api.md) — **`metrics`** vs **`txn`**
- [Config schema: metrics](/reference/config-schema/metrics-and-tracing.md)
- [Rhai for rules](/rhai/rule-rhai.md)
