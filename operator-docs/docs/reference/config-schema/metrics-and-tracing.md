# Config schema: metrics and tracing

Field reference for the optional top-level **`metrics:`** and **`tracing:`** blocks. For operator guides, see [Metrics](/observability/metrics.md) and [Tracing](/observability/tracing.md).

Both blocks are **file-layer only** — [overlay](/glossary/index.md#overlay) patches that include `metrics` or `tracing` are rejected.

## `metrics`

| Property | Value |
|----------|--------|
| **Type** | Object |
| **Required** | No — when omitted, built-in metrics are off |
| **Location** | Top-level key in the [config file](/control-plane/config-file.md) |

| Field {: .column-no-wrap } | Type | Required | Default | Description |
|-------|------|----------|---------|-------------|
| `enabled` | boolean | no | **`false`** | Must be **`true`** for hot-path recording |
| `profile` | string | no | **`full`** when enabled | **`minimal`**, **`full`**, or **`off`** |
| `prometheus` | object | no | — | HTTP scrape listener |
| `otel` | object | no | — | OTLP HTTP metrics push |
| `user_metrics` | list | no | `[]` | Per-metric export tier overrides for Rhai `conduit_user_*` counters |

### `metrics.user_metrics[]`

| Field | Type | Required | Default | Description |
|-------|------|----------|---------|-------------|
| `name` | string | yes | — | Metric name from `metric_inc` (without `conduit_user_` prefix) |
| `export` | string | no | **`full`** | **`minimal`** (record on `minimal` and `full` profiles) or **`full`** (`full` profile only) |

Script-discovered metrics default to **`export: full`**. Override to **`minimal`** to record on the [minimal built-in profile](/observability/built-in-metrics.md#profiles). See [User metrics](/rhai/user-metrics.md#export-tier).

### `metrics.prometheus`

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `listen_address` | string | — | Bind address for scrape (for example `127.0.0.1:9090`) |
| `path` | string | **`/metrics`** | HTTP path for scrape |

### `metrics.otel`

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `endpoint` | string | — | OTLP HTTP URL (must start with `http://` or `https://`; typically `/v1/metrics`) |
| `push_interval_ms` | integer | **15000** | Push period; minimum **1000** when set |
| `allow_invalid_certs` | boolean | **`false`** | Accept invalid TLS server certs for **`https://`** endpoints |
| `resource_attributes` | map | `{}` | Resource labels attached to pushed metrics |
| `headers` | map | `{}` | OTLP HTTP headers (wired in code; operator auth story still evolving) |

## `tracing`

| Property | Value |
|----------|--------|
| **Type** | Object |
| **Required** | No — when omitted, pipeline tracing is off |
| **Location** | Top-level key in the [config file](/control-plane/config-file.md) |

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
| `metrics.profile` when enabled | Must be **`full`**, **`minimal`**, **`off`**, or empty (defaults to **`full`**) |
| `metrics.prometheus.listen_address` | Must parse as socket address when non-empty |
| `metrics.otel.endpoint` | Must be `http://` or `https://` when non-empty |
| `metrics.otel.push_interval_ms` | Must be **≥ 1000** when non-zero |
| `metrics.user_metrics[].name` | Must be non-empty; must match a Rhai-registered metric at snapshot build |
| `metrics.user_metrics[].export` | Must be **`minimal`**, **`full`**, or empty (defaults to **`full`**) |
| Duplicate `metrics.user_metrics[].name` | Rejected |
| `tracing.activation.sample_percent` | Must be in **[0, 100]** |
| Selector `type` in `tracing.activation.selectors` | Must be a known selector type |
| `metrics` or `tracing` in overlay patch | Overlay rejected |

Validate with `conduitctl validate --file …` or load via the running process; see [Config file](/control-plane/config-file.md).

## Example configuration

```yaml
metrics:
  enabled: true
  profile: minimal
  user_metrics:
    - name: block_hits
      export: minimal
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

- [Metrics](/observability/metrics.md) — profiles, scrape, OTEL push, restart semantics
- [Tracing](/observability/tracing.md) — activation, GetTrace, trace events
- [Built-in metrics](/observability/built-in-metrics.md) — exported series catalog
- [Configuration model](/control-plane/configuration-model.md) — file layer vs overlay
