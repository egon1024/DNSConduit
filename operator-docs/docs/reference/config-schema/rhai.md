# Config schema: rhai

This page lists the fields for the optional top-level **`rhai:`** block (sandbox limits for [Rule Rhai](/rhai/rule-rhai.md)). For what each limit means, fail-open behavior, host API costing, and safe patterns, see [Sandbox limits](/rhai/sandbox-limits.md).

## `rhai`

| Property | Value |
|----------|--------|
| **Type** | Object |
| **Required** | No — when omitted, built-in defaults still apply |
| **Location** | Top-level key in the [config file](/control-plane/config-file.md) or [overlay](/glossary/index.md#overlay) (whole-section replace) |

| Field {: .column-no-wrap } | Type | Required | Default | Description |
|-------|------|----------|---------|-------------|
| `max_operations` | integer | no | **10000** | Rhai operations budget per script invocation. Must be **≥ 1** when set (**`0` fails validation**) |
| `max_call_depth` | integer | no | **32** | Maximum call-stack depth for Rhai functions. Must be **≥ 1** when set |
| `hook_timeout_ms` | integer | no | **50** | Wall-clock limit in milliseconds for one script run. **`0` in YAML means use default 50** — not unlimited |

```yaml
rhai:
  max_operations: 10000
  max_call_depth: 32
  hook_timeout_ms: 50
```

Limits apply to **Rule Rhai only**. They take effect on the next successful reload or apply — no process restart required. See [Sandbox limits — Reload and validation](/rhai/sandbox-limits.md#reload-and-validation).

## Validation

| Rule | Error if violated |
|------|-------------------|
| `max_operations` ≥ **1** when present | `rhai.max_operations must be >= 1 when set` |
| `max_call_depth` ≥ **1** when present | `rhai.max_call_depth must be >= 1 when set` |

YAML field checks run on `conduitctl validate`, startup, and reload. Script syntax, missing files, unknown `lookup` literals, and metric registration are checked later at **snapshot compile** — see [Sandbox limits](/rhai/sandbox-limits.md).

## Related topics

- [Sandbox limits](/rhai/sandbox-limits.md) — behavior, fail-open, observability
- [Rules](/reference/config-schema/rules.md) — `type: rhai` actions
- [Data sources](/reference/config-schema/data-sources.md) — tables for `lookup(table, key)`
- [Config file](/control-plane/config-file.md)
