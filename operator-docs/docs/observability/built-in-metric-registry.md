# Built-in metric registry

Each built-in metric **family** has a category, a recording path, and a label-granularity scheme. Live scrapes only show series enabled in the active metrics plan — absence from scrape means the family is not in that plan, not that the product lacks the metric.

For scrape/OTLP setup and collect vs emit cost, see [Metrics configurability](/observability/metrics-configurability.md). For per-series increment rules and PromQL, see [Built-in metrics](/observability/built-in-metrics.md).

## How to read this table

| Column {: .column-no-wrap } | Meaning |
|--------|---------|
| **Category** | Dataplane category; included via `base` and `categories.include` / `exclude` |
| **minimal** / **standard** | Whether the category (hence the family) is on for that base |
| **Timing** | Hot-path (query workers) vs scrape-time (export refresh) |
| **Dimensions** | Label keys controlled by `granularity` presets / overrides (empty = no plan dimensions beyond fixed labels) |

Default collect/emit when a category is on: both **true**. Override with `metrics.collection.<category>`.

Opt-in-only families (none on day one) appear here when added; they stay off until explicitly included.

## Category membership

| Category {: .column-no-wrap } | `minimal` | `standard` | Typical series |
|----------|-----------|------------|-------|
| `volume` | <span class="membership-yes">yes</span> | <span class="membership-yes">yes</span> | Queries, responses, truncations, drops, ACL, queries-by-pool |
| `failures` | <span class="membership-yes">yes</span> | <span class="membership-yes">yes</span> | Parse / forward / script errors, retries, slot exhaustion |
| `lookup` | <span class="membership-yes">yes</span> | <span class="membership-yes">yes</span> | Lookup outcomes and cache lookups |
| `timing` | <span class="membership-no">no</span> | <span class="membership-yes">yes</span> | Duration histograms and forward attempts |
| `cache_detail` | <span class="membership-no">no</span> | <span class="membership-yes">yes</span> | Cache fills, singleflight, entries / evictions |
| `forward_detail` | <span class="membership-no">no</span> | <span class="membership-yes">yes</span> | Forward outstanding gauge (scrape) |
| `health` | <span class="membership-yes">yes</span> | <span class="membership-yes">yes</span> | Probe results and backend health gauges (**included in minimal**) |
| `runtime` | <span class="membership-no">no</span> | <span class="membership-yes">yes</span> | Slot in-use / capacity gauges |
| `topology` | <span class="membership-yes">yes</span> | <span class="membership-yes">yes</span> | Configured backends, listener / backend info |
| `process` | <span class="membership-no">no</span> | <span class="membership-yes">yes</span> | Process RSS, FDs, threads, CPU (Linux scrape) |
| `meta` | <span class="membership-yes">yes</span> | <span class="membership-yes">yes</span> | Build info, start time, config generation |

## Families by category

### volume

| Family | Timing | Typical dimensions (fine) |
|--------|--------|---------------------------|
| `conduit_queries_total` | hot | `listener`, `protocol`, `qtype`, `qclass`, `ip_family` |
| `conduit_queries_by_pool_total` | hot | `pool` |
| `conduit_queries_dropped_total` | hot | `listener`, `protocol`, `reason`, `ip_family` |
| `conduit_responses_total` | hot | `listener`, `protocol`, `rcode`, `answer_source`, `ip_family` |
| `conduit_responses_truncated_total` | hot | `listener`, `protocol`, `answer_source`, `ip_family` |
| `conduit_acl_decisions_total` | hot | `tier`, `action`, `listener`, `ip_family` |

Coarse presets drop high-cardinality labels (for example qtype / ip_family). Response **`rcode`** values use coarse class buckets or IANA names via `granularity.responses`.

### failures

| Family | Timing | Typical dimensions (fine) |
|--------|--------|---------------------------|
| `conduit_parse_rejected_total` | hot | `reason` |
| `conduit_forward_errors_total` | hot | `pool`, `backend`, `reason` |
| `conduit_retries_total` | hot | `pool` |
| `conduit_script_errors_total` | hot | `reason`, `script`, `table` |
| `conduit_slot_pool_exhausted_total` | hot | |

### lookup

| Family | Timing | Typical dimensions (fine) |
|--------|--------|---------------------------|
| `conduit_lookup_provider_outcomes_total` | hot | `profile`, `provider`, `outcome` |
| `conduit_cache_lookups_total` | hot | `cache`, `profile`, `result` |

### timing

| Family | Timing | Typical dimensions (fine) |
|--------|--------|---------------------------|
| `conduit_phase_duration_seconds` | hot | `phase` |
| `conduit_forward_duration_seconds` | hot | `pool`, `backend` |
| `conduit_forward_attempts_total` | hot | `pool`, `backend`, outcome |
| `conduit_lookup_duration_seconds` | hot | |
| `conduit_cache_duration_seconds` | hot | |
| `conduit_response_duration_seconds` | hot | |

Pool-only overrides (for example `granularity.timing: [pool]`) drop `backend` where applicable.

### cache_detail / forward_detail / health / runtime / topology / process / meta

| Family | Category | Timing |
|--------|----------|--------|
| Cache fill / singleflight / entries / evictions series | `cache_detail` | hot / scrape as documented in [Built-in metrics](/observability/built-in-metrics.md) |
| `conduit_forward_outstanding` | `forward_detail` | scrape |
| `conduit_probe_results_total`, `conduit_backend_health_observed`, related health gauges | `health` | hot / scrape |
| Slot in-use / capacity gauges | `runtime` | scrape |
| `conduit_pool_backends_configured`, listener / backend info | `topology` | scrape |
| `conduit_process_resident_bytes`, `conduit_process_open_fds`, `conduit_process_max_fds`, `conduit_process_threads`, `conduit_process_cpu_seconds_total` | `process` | scrape (Linux) |
| `conduit_build_info`, `conduit_start_time_seconds`, `conduit_uptime_seconds`, `conduit_config_generation` | `meta` | scrape |

Exact label sets and PromQL: [Built-in metrics](/observability/built-in-metrics.md).

## Event export (separate axis)

| Family prefix | Controlled by | Notes |
|---------------|---------------|-------|
| `conduit_events_*` | `metrics.event_export.{collect,emit}` | Not a dataplane category; see [Event export](/observability/event-export.md) |

## Rhai user metrics

| Pattern | Controlled by |
|---------|---------------|
| `conduit_user_<name>` | Script registration + `metrics.user_metrics[]` collect/emit (defaults follow standard-tier semantics) |

See [User metrics](/rhai/user-metrics.md).

## Related topics

- [Metrics configurability](/observability/metrics-configurability.md)
- [Built-in metrics](/observability/built-in-metrics.md)
- [Metrics](/observability/metrics.md)
