# Config schema: logging

Field reference for the optional top-level **`logging:`** block. For log format, choosing a level, `RUST_LOG`, and lab smoke tests, see [Logging](/observability/logging.md).

## `logging`

| Property | Value |
|----------|--------|
| **Type** | Object |
| **Required** | No — when omitted, **`info`** on **stderr** |
| **Location** | Top-level key in the [config file](/control-plane/config-file.md) or [overlay](/glossary/index.md#overlay) (whole-section replace) |

| Field {: .column-no-wrap } | Type | Required | Default | Description |
|-------|------|----------|---------|-------------|
| `level` | string | no | **`info`** | Minimum severity: **`error`**, **`warn`**, **`info`**, **`debug`**, or **`trace`** |
| `output` | string | no | **`stderr`** | **`stderr`** or **`stdout`** |

```yaml
logging:
  level: info
  output: stderr
```

## Validation

| Rule | Error if violated |
|------|-------------------|
| Valid `level` | Rejected at validate / load |
| Valid `output` | Rejected at validate / load |

Validate with `conduitctl validate --file …`.

## Reload and restart

The log subscriber is initialized **once at process start**. Changing **`level`** or **`output`** via reload or overlay updates the stored config document but does **not** rebind the active subscriber — **restart** the process for the new values to take effect.

If **`RUST_LOG`** is set at process start, it **replaces** `logging.level` for filter construction. See [Logging — `RUST_LOG` override](/observability/logging.md#rust_log-override).

!!! note "`logging.level: trace` is not pipeline tracing"
    **`trace`** here is maximum **log verbosity**. Per-query [pipeline traces](/glossary/index.md#pipeline-trace) use the separate **`tracing:`** block — see [Config schema: metrics and tracing](/reference/config-schema/metrics-and-tracing.md) and [Tracing](/observability/tracing.md).

## Related topics

- [Logging](/observability/logging.md) — format, levels, representative lines, lab smoke test
- [Config file](/control-plane/config-file.md)
- [Configuration model](/control-plane/configuration-model.md) — file layer vs overlay
