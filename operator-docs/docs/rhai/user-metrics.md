---
toc_depth: 3
---

# User metrics

Rhai scripts publish custom counters through the **`metrics`** scope object: **`metrics.inc`** and **`metrics.inc_labels`**. Exported series use the prefix **`conduit_user_<name>`** (for example `conduit_user_block_hits`).

**`metrics`** is separate from **`txn`** — counters are not per-query policy state. See [Host API overview](/rhai/host-api.md) for how the scopes fit together.

Built-in [metrics profiles](/observability/metrics.md#profiles) (`minimal` / `full`) also control **whether** each user metric is recorded on the hot path. See [Export tier](#export-tier) below.

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

## Export tier { #export-tier }

Each user metric has an **export tier** that decides when increments reach the Prometheus/OTEL registry:

| Tier | Recorded when `metrics.profile` is… |
|------|-------------------------------------|
| **`full`** (default) | **`full`** only |
| **`minimal`** | **`minimal`** or **`full`** |

Unlisted script-discovered metrics default to **`full`**. On a **`minimal`** deployment, they are **not** recorded unless you opt them in under **`metrics.user_metrics`**.

`metrics.inc` still succeeds when a metric is filtered out — increments are dropped silently at export (no script error).

### Config overrides

```yaml
metrics:
  enabled: true
  profile: minimal
  user_metrics:
    - name: block_hits
      export: minimal
```

| Field | Meaning |
|-------|---------|
| `name` | Metric name from `metrics.inc` (without `conduit_user_` prefix) |
| `export` | **`minimal`** or **`full`** (empty = **`full`**) |

Validation:

- Each `name` must match a metric registered by at least one Rhai script at compile time.
- Duplicate `name` entries are rejected.
- Unknown names fail snapshot build (`conduitctl validate`).

Fixture: `tests/fixtures/config/with-rhai-block-hits-minimal-export.yaml`.

## Export path

When `metrics.enabled` is true, successful hook runs flush allowed user-metric deltas into the process-wide user registry. [Prometheus scrape](/observability/metrics.md) and [OTEL push](/observability/metrics.md) include `conduit_user_*` series alongside built-ins.

Recording does not require an export listener — counters accumulate in memory. Configure `prometheus` and/or `otel` to observe them externally.

## Examples

| Script | Config | Notes |
|--------|--------|-------|
| `block-hits.rhai` | `with-rhai-block-hits.yaml` | `block_hits` with `category` label; **`full`** profile |
| `block-hits.rhai` | `with-rhai-block-hits-minimal-export.yaml` | `block_hits` opted into **`minimal`** |
| `slow-login-alert.rhai` | `with-rhai-slow-login.yaml` | `slow_login` when `txn.last_forward_ms() > 500` |

## Cache hits and on_hit skip { #cache-hits-and-on_hit-skip }

Response-hook **`metrics.inc`** runs only when [Response rules](/concepts/architecture-and-packet-path.md#response-rules) run. On a **cache hit** with **`on_hit.response_rules: skip`**, the response hook is **not** invoked — custom counters on that hook will not increment.

Options:

- Keep default **`on_hit.response_rules: run`** so response rules and metrics run on hits
- Use built-in [`conduit_responses_total`](/observability/built-in-metrics.md#conduit_responses_total) with **`answer_source`**
- Record metrics on the **request** hook when classification is enough

See [DNS answer cache — on_hit tradeoff](/guides/dns-answer-cache.md#on_hit-response_rules).

## Related topics

- [Built-in metrics — User metrics](/observability/built-in-metrics.md#user-metrics-rhai)
- [Host API overview](/rhai/host-api.md) — **`metrics`** vs **`txn`**
- [Config schema: metrics](/reference/config-schema/metrics-and-tracing.md)
- [Rhai for rules](/rhai/rule-rhai.md)
