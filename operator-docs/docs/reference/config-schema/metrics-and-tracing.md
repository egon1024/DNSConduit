# Config schema: metrics and tracing

Field reference for the optional top-level **`metrics:`** and **`tracing:`** blocks. Guides: [Metrics](/observability/metrics.md), [Metrics configurability](/observability/metrics-configurability.md), [Tracing](/observability/tracing.md).

**`metrics`** may appear in [overlay](/glossary/index.md#overlay) patches ([deep merge](/control-plane/overlay-merge-strategy.md#metrics-deep-merge)). **`tracing`** remains file-layer only.

## `metrics`

| Property | Value |
|----------|--------|
| **Type** | Object |
| **Required** | No — when omitted, built-in metrics are off |
| **Location** | Top-level key in the [config file](/control-plane/config-file.md); also allowed in overlay |

| Field {: .column-no-wrap } | Type | Required | Default | Description |
|-------|------|----------|---------|-------------|
| `enabled` | boolean | no | **`false`** | Must be **`true`** for hot-path recording |
| `base` | string | no | **`standard`** when enabled | **`none`**, **`minimal`**, or **`standard`** |
| `profile` | string | no | — | **Deprecated alias** (kept through 1.x): `minimal` → `base: minimal`; `full` → `base: standard`; `off` → `enabled: false` |
| `categories` | object | no | — | `include` / `exclude` lists applied after `expand(base)` |
| `collection` | map | no | — | Per-category `collect` / `emit` overrides |
| `granularity` | object | no | from base | `default` (`coarse` \| `balanced` \| `fine`) plus per-family dimension lists / responses rcode |
| `event_export` | object | no | collect+emit true | Controls `conduit_events_*` (not a dataplane category) |
| `prometheus` | object | no | — | HTTP scrape listener |
| `otel` | object | no | — | OTLP HTTP metrics push |
| `user_metrics` | list | no | `[]` | Per-metric collect/emit (and deprecated `export`) for Rhai `conduit_user_*` |

Validation highlights: empty resolved category set while enabled → error; `collect: false` with `emit: true` → error; stopping collect for a user metric still referenced by a Rhai script → error (script path listed). See [Metrics configurability](/observability/metrics-configurability.md).

### `metrics.categories`

| Field | Type | Description |
|-------|------|-------------|
| `include` | list of string | Category names added to the base expansion |
| `exclude` | list of string | Category names removed after include |

### `metrics.collection` / `metrics.event_export` / `metrics.user_metrics[]`

| Field | Type | Description |
|-------|------|-------------|
| `collect` | boolean | Record into the process store |
| `emit` | boolean | Include in Prometheus / OTLP |
| `name` | string | (`user_metrics` only) bare metric name |
| `export` | string | **Deprecated** (`minimal` \| `full`); prefer collect/emit |

### `metrics.prometheus`

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `listen_address` | string | — | Bind address for scrape (for example `127.0.0.1:9090`) |
| `path` | string | **`/metrics`** | HTTP path for scrape |

Hot-rebinds on apply when address or path changes; bind failure rejects apply.

### `metrics.otel`

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `endpoint` | string | — | OTLP HTTP URL (must start with `http://` or `https://`; typically `/v1/metrics`) |
| `push_interval_ms` | integer | **15000** | Push period; minimum **1000** when set |
| `allow_invalid_certs` | boolean | **`false`** | Accept invalid TLS server certs for **`https://`** endpoints |
| `resource_attributes` | map | `{}` | Resource labels attached to pushed metrics |
| `headers` | map | `{}` | HTTP headers sent with each OTLP metrics push (for example `Authorization: Bearer …`) |

## `tracing`

| Property | Value |
|----------|--------|
| **Type** | Object |
| **Required** | No — when omitted, pipeline tracing is off |
| **Location** | Top-level key in the [config file](/control-plane/config-file.md) (file-layer only) |

| Field {: .column-no-wrap } | Type | Required | Default | Description |
|-------|------|----------|---------|-------------|
| `enabled` | boolean | no | **`false`** | Must be **`true`** to evaluate activation and record traces |
| `activation` | object | no | (match all) | [Trace activation](#trace-activation-object) filters |
| `output` | object | no | — | [Trace output](#trace-output-object) |

### Trace activation object

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `tag` | string | — | Transaction must have this tag key |
| `selectors` | list | `[]` | [Selector](/glossary/index.md#selector) objects; same types as [rules](/reference/config-schema/rules.md). All must match |
| `sample_percent` | float | **100** | Must be in **[0, 100]**; deterministic sampling |
| `sample_key` | string | — | Optional static salt for `sample_percent` |
| `sample_key_from` | string | — | Optional `qname` for `sample_percent` |

Evaluated after [Request rules](/concepts/architecture-and-packet-path.md#request-rules). See [Tracing — Activation](/observability/tracing.md#activation).

### Trace output object

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `log_json` | boolean | **`false`** | Log completed traces as JSON at **`info`** (`conduit::trace`) |

## Validation summary

| Rule | Error if violated |
|------|-------------------|
| `metrics.base` when enabled | Must be **`none`**, **`minimal`**, **`standard`**, or empty (defaults to **`standard`**) |
| `metrics.profile` (alias) | Must be **`minimal`**, **`full`**, **`off`**, or empty |
| Resolved category set empty while enabled | Rejected |
| `collect: false` with `emit: true` (category, event_export, or user metric) | Rejected |
| User metric collect removed while Rhai still references it | Rejected (error lists script path) |
| `metrics.prometheus.listen_address` | Must parse as socket address when non-empty |
| `metrics.otel.endpoint` | Must be `http://` or `https://` when non-empty |
| `metrics.otel.push_interval_ms` | Must be **≥ 1000** when non-zero |
| `metrics.user_metrics[].name` | Must be non-empty; must match a Rhai-registered metric at snapshot build |
| Duplicate `metrics.user_metrics[].name` | Rejected |
| `tracing.activation.sample_percent` | Must be in **[0, 100]** |
| Selector `type` in `tracing.activation.selectors` | Must be a known selector type |
| `tracing` in overlay patch | Overlay rejected |

Validate with `conduitctl validate --file …` or load via the running process; see [Config file](/control-plane/config-file.md).

## Example configuration

```yaml
metrics:
  enabled: true
  base: minimal
  user_metrics:
    - name: block_hits
      collect: true
      emit: true
  prometheus:
    listen_address: "127.0.0.1:9090"
    path: /metrics
  otel:
    endpoint: "http://127.0.0.1:4318/v1/metrics"
    push_interval_ms: 15000
    resource_attributes:
      service.name: conduit

tracing:
  enabled: true
  activation:
    selectors:
      - type: qtype
        value: A
    sample_percent: 100
  output:
    log_json: false
```

## Related topics

- [Metrics configurability](/observability/metrics-configurability.md) — bases, categories, collect/emit, overlay
- [Metrics](/observability/metrics.md) — scrape and OTEL push
- [Tracing](/observability/tracing.md) — activation, GetTrace, trace events
- [Built-in metric registry](/observability/built-in-metric-registry.md)
- [Built-in metrics](/observability/built-in-metrics.md) — exported series catalog
- [Overlay merge strategy](/control-plane/overlay-merge-strategy.md)
- [Configuration model](/control-plane/configuration-model.md) — file layer vs overlay
