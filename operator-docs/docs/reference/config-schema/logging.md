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
| `query_access` | object | no | omitted | Per-event levels for [Client ACL](/policy-routing/client-acls.md) denials (and related access lines). Does **not** require raising global `logging.level`. |

### `logging.query_access`

| Field | Type | Required | Default | Description |
|-------|------|----------|---------|-------------|
| `acl_denied` | string | no | **`off`** | Level for ACL denial lines: **`off`**, **`error`**, **`warn`**, **`info`**, **`debug`**, **`trace`**. Default stays quiet at global **`info`**. |
| `acl_denied_sample` | object | no | omitted (log all at configured level) | Optional sampling — does **not** affect metrics or enforcement |

#### `acl_denied_sample`

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `mode` | string | yes | **`per_source`** (hash of client IP) or **`every_nth`** (per-worker counter) |
| `rate` | number | for `per_source` | 0–100 — percent of distinct client IPs that emit deny logs |
| `nth` | integer | for `every_nth` | Emit one of every N denials on this worker (`N >= 1`) |

```yaml
logging:
  level: info
  output: stderr
  query_access:
    acl_denied: warn
    acl_denied_sample:
      mode: every_nth
      nth: 100
```

Denial lines include client IP, listener, matched view (or `default`), action, enforcement stage (`preadmission` / `listener`), and IP family. Prefer **`warn`** or **`debug`** for expected deny volume — not **`error`**.

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
