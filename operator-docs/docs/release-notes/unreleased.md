# Unreleased

## Packaging

- **Guide lab examples:** Production tarballs and the `conduit` `.deb` ship primary runnable configs from the [Guides](/guides/index.md) under `examples/` (package path `/usr/share/doc/conduit/examples/`), including health, cache, failover, rule action order, metrics labs, dnstap, and the Rhai blocklist tree. See [Install and run](/getting-started/install-and-run.md).

## Metrics configurability

- **Bases and categories:** Prefer `metrics.base` (`none` | `minimal` | `standard`) with optional `categories.include` / `exclude`. `standard` is a curated bundle, not the entire registry. A category listed in both include and exclude emits a warning and exclude wins. See [Metrics configurability](/observability/metrics-configurability.md) and [Built-in metric registry](/observability/built-in-metric-registry.md).
- **Collect vs emit:** Per-category and per-user-metric `collect` / `emit`. Collect-only still costs hot-path recording; it only skips export.
- **Granularity:** `granularity.default` and per-family dimension lists; response rcode coarse vs IANA.
- **Event export axis:** `metrics.event_export.{collect,emit}` for `conduit_events_*`.
- **Overlay:** `metrics` allowed with [deep merge](/control-plane/overlay-merge-strategy.md). Plan changes apply live; Prometheus listen **rebinds**; OTLP **reconnects**; bind/reconnect failure rejects apply and keeps last-good.
- **Consumer validation:** Write sites (`metrics.inc` / `inc_labels`) with collect or emit off warn (increments no-op / series stay out of export) but validate/apply succeed — same model as built-in categories. On **`base: minimal`**, unlisted script metrics default to collect off; opt them in under **`user_metrics`** (or use **`base: standard`**) when you want them recorded. Future read APIs will reject collect-off while they still reference a metric. See [User metrics](/rhai/user-metrics.md).
- **User metric HELP:** Optional `metrics.user_metrics[].help` sets Prometheus `# HELP` and the OTel instrument description for `conduit_user_*` (default remains **Rhai user-defined metric** when omitted).
- **Minimal includes health:** `base: minimal` includes the `health` category (probe / backend health series when health is configured).
- **Process metrics:** `process` category (`standard`) exposes Linux scrape series for RSS, open/max FDs, thread count, and CPU seconds (`conduit_process_*`). See [Built-in metrics — Process and build](/observability/built-in-metrics.md#process-and-build).
- **Uptime:** `meta` includes [`conduit_uptime_seconds`](/observability/built-in-metrics.md#conduit_uptime_seconds) (monotonic seconds since process start, scrape-refreshed). Prefer it over deriving uptime from [`conduit_start_time_seconds`](/observability/built-in-metrics.md#conduit_start_time_seconds) when wall clocks may jump.

### Upgrade from `metrics.profile`

Prefer **`metrics.base`** and **`collect` / `emit`**. Mapping for former **`profile`** and **`user_metrics[].export`**: [Metrics configurability — Legacy aliases](/observability/metrics-configurability.md#legacy-profile-alias). Aliases remain accepted through the **1.x** line (deprecation warnings); removal is scheduled for the **2.x** breaking line.
