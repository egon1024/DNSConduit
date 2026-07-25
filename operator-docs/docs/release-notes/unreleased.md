# Unreleased

## Metrics configurability

- **Bases and categories:** Prefer `metrics.base` (`none` | `minimal` | `standard`) with optional `categories.include` / `exclude`. `standard` is a curated bundle, not the entire registry. See [Metrics configurability](/observability/metrics-configurability.md) and [Built-in metric registry](/observability/built-in-metric-registry.md).
- **Collect vs emit:** Per-category and per-user-metric `collect` / `emit`. Collect-only still costs hot-path recording; it only skips export.
- **Granularity:** `granularity.default` and per-family dimension lists; response rcode coarse vs IANA.
- **Event export axis:** `metrics.event_export.{collect,emit}` for `conduit_events_*`.
- **Overlay:** `metrics` allowed with [deep merge](/control-plane/overlay-merge-strategy.md). Plan changes apply live; Prometheus listen **rebinds**; OTLP **reconnects**; bind/reconnect failure rejects apply and keeps last-good.
- **Consumer validation:** Stopping collect for a Rhai-referenced user metric fails validate/apply; the error lists the script path.
- **Minimal includes health:** `base: minimal` includes the `health` category (probe / backend health series when health is configured).

### Upgrade from `metrics.profile`

| Former | Prefer |
|--------|--------|
| `profile: minimal` | `base: minimal` |
| `profile: full` | `base: standard` |
| `profile: off` (with enabled) | `enabled: false` |
| `user_metrics[].export` | `collect` / `emit` |

`profile` and `export` remain accepted aliases through the **1.x** line (deprecation warnings). Removal is scheduled for the **2.x** breaking line. Configs with no new keys keep the same series identity as the former `profile: full` / default standard plan when granularity is unset.
