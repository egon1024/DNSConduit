---
toc_depth: 3
toc_collapsible: true
---

# Built-in metrics

Conduit exports **built-in** Prometheus series (not Rhai `conduit_user_*` metrics) with fixed names and labels. For enabling scrape, profiles, and OTEL push, see [Metrics](/observability/metrics.md). For how metrics map to the query path, see [Architecture and packet path](/concepts/architecture-and-packet-path.md).

Built-in labels never include `qname`, client IP, or transaction id — use [event export](/observability/event-export.md) or [tracing](/observability/tracing.md) for per-name detail.

The **`backend`** label on forward and health-probe metrics ([`conduit_forward_attempts_total`](#conduit_forward_attempts_total), [`conduit_forward_errors_total`](#conduit_forward_errors_total), [`conduit_forward_duration_seconds`](#conduit_forward_duration_seconds), [`conduit_forward_outstanding`](#conduit_forward_outstanding), [`conduit_probe_results_total`](#conduit_probe_results_total), and the [`conduit_backend_health_*`](#backend-health) series) is the backend [`name`](/policy-routing/pools-and-backends.md#backend-names) when one is set, otherwise its `address`. Naming a backend keeps its time series stable across an address change; see [Backend names](/policy-routing/pools-and-backends.md#backend-names).

## Profiles { #profiles }

Prefer **`metrics.base`** (`minimal` / `standard`) — see [Metrics configurability](/observability/metrics-configurability.md) and [Built-in metric registry](/observability/built-in-metric-registry.md). Tables on this page still say **`minimal`** vs **`full`** for label richness: **`full`** means the fine / **`base: standard`** schema (same identity as former `profile: full` when granularity is unset).

When the `metrics:` block is omitted, built-ins are off — no hot-path increments and no export. Base / categories / collect·emit choose **what** Conduit records; Prometheus and OTEL choose **how** you export it.

### `minimal` vs `full` (hot path)

Both bases record metrics **while handling queries** on listener workers (the **hot path**). The difference is **how much** is recorded and **how many label dimensions** are kept — a cardinality and overhead trade-off.

**`minimal`** — low-cardinality volume and essential failure counters (plus health / topology / meta per the registry):

- [`conduit_queries_total`](#conduit_queries_total) with `listener` and `protocol` only
- [`conduit_queries_by_pool_total`](#conduit_queries_by_pool_total) per `pool`
- [`conduit_responses_total`](#conduit_responses_total) with coarse `rcode` buckets
- [`conduit_responses_truncated_total`](#conduit_responses_truncated_total) — UDP send-path truncation (joinable with responses)
- Failure counters: [`conduit_parse_rejected_total`](#conduit_parse_rejected_total), [`conduit_queries_dropped_total`](#conduit_queries_dropped_total), [`conduit_forward_errors_total`](#conduit_forward_errors_total), [`conduit_retries_total`](#conduit_retries_total), [`conduit_script_errors_total`](#conduit_script_errors_total)

Use **`minimal`** when you want query volume, pool mix, response mix, and alertable failure signals without per-qtype detail, forward latency histograms, or per-phase timing on the hot path.

**`full`** / **`standard`** — complete built-in observability on the hot path:

- Richer [`conduit_queries_total`](#conduit_queries_total) labels (`qtype`, `qclass`, `ip_family`)
- Fine [`conduit_responses_total`](#conduit_responses_total) `rcode` labels and `ip_family`
- Forward attempt counts, forward RTT histograms, per-phase timing histograms (see table below)
- Transaction slot-pool gauges ([`conduit_slots_in_use`](#conduit_slots_in_use), [`conduit_slots_capacity`](#conduit_slots_capacity)) at scrape time
- Linux process series ([`conduit_process_resident_bytes`](#conduit_process_resident_bytes), [`conduit_process_open_fds`](#conduit_process_open_fds), [`conduit_process_max_fds`](#conduit_process_max_fds), [`conduit_process_threads`](#conduit_process_threads), [`conduit_process_cpu_seconds_total`](#conduit_process_cpu_seconds_total)) at scrape time

Both **`minimal`** and **`standard`** include the **`health`** category: when pool health is enabled, probe outcome counts ([`conduit_probe_results_total`](#conduit_probe_results_total)) and health gauges export on either base.

Use **`standard`** for day-two operations, SLO dashboards, and debugging upstream or pipeline behavior. Built-in labels still never include `qname`, client IP, or transaction id at either base.

**Scrape-time** series ([Scrape-time gauges](#scrape-time-gauges)) depend on category membership: slot-pool and process series require categories present on **`standard`** (not on **`minimal`**). Health gauges are available on **`minimal`** and **`standard`** when health is configured. The slot exhaustion counter ([`conduit_slot_pool_exhausted_total`](#conduit_slot_pool_exhausted_total)) is in the **failures** category (both bases).

| Series | Hot path `minimal` | Hot path `full` | [Scrape-time](#scrape-time-gauges) only |
|--------|-------------------|-----------------|-------------|
| [`conduit_queries_total`](#conduit_queries_total) | `listener`, `protocol` | + `qtype`, `qclass`, `ip_family` | — |
| [`conduit_queries_by_pool_total`](#conduit_queries_by_pool_total) | yes (`pool`) | yes | — |
| [`conduit_queries_dropped_total`](#conduit_queries_dropped_total) | yes (`listener`, `protocol`, `reason`) | yes (+ `ip_family`) | — |
| [`conduit_parse_rejected_total`](#conduit_parse_rejected_total) | yes (`reason`) | yes | — |
| [`conduit_acl_decisions_total`](#conduit_acl_decisions_total) | yes (`tier`, `action`, `listener`) | yes (+ `ip_family`) | — |
| [`conduit_responses_total`](#conduit_responses_total) | yes (`listener`, `protocol`, coarse `rcode`, `answer_source`) | yes (+ fine `rcode`, `ip_family`) | — |
| [`conduit_responses_truncated_total`](#conduit_responses_truncated_total) | yes (`listener`, `protocol`, `answer_source`) | yes (+ `ip_family`) | — |
| [`conduit_forward_errors_total`](#conduit_forward_errors_total) | yes (`pool`, `backend`, `reason`) | yes | — |
| [`conduit_retries_total`](#conduit_retries_total) | yes (`pool`) | yes | — |
| [`conduit_script_errors_total`](#conduit_script_errors_total) | yes (`reason`, `script`, `table`) | yes | — |
| Phase / forward-attempt / forward-duration / lookup-cache histograms below | no | yes | — |
| [`conduit_lookup_provider_outcomes_total`](#conduit_lookup_provider_outcomes_total), [`conduit_cache_lookups_total`](#conduit_cache_lookups_total) | yes | yes | — |
| [`conduit_cache_fills_total`](#conduit_cache_fills_total), [`conduit_cache_singleflight_coalesced_total`](#conduit_cache_singleflight_coalesced_total), lookup/cache duration histograms | no | yes | — |
| [`conduit_probe_results_total`](#conduit_probe_results_total) | yes (health enabled) | yes | — |
| [`conduit_forward_outstanding`](#conduit_forward_outstanding) | — | — | yes (`standard`) |
| [`conduit_pool_backends_configured`](#conduit_pool_backends_configured) | — | — | yes |
| Backend health gauges ([`conduit_backend_health_*`](#backend-health), [`conduit_pool_backends_active`](#conduit_pool_backends_active)) | — | — | yes (`minimal` and `standard`, health enabled) |
| [`conduit_slots_in_use`](#conduit_slots_in_use), [`conduit_slots_capacity`](#conduit_slots_capacity) | — | — | yes (`standard` / `full`) |
| [`conduit_slot_pool_exhausted_total`](#conduit_slot_pool_exhausted_total) | — | — | yes |
| [`conduit_build_info`](#conduit_build_info), [`conduit_start_time_seconds`](#conduit_start_time_seconds), [`conduit_uptime_seconds`](#conduit_uptime_seconds), [`conduit_config_generation`](#conduit_config_generation) | — | — | yes |
| [`conduit_process_resident_bytes`](#conduit_process_resident_bytes), [`conduit_process_open_fds`](#conduit_process_open_fds), [`conduit_process_max_fds`](#conduit_process_max_fds), [`conduit_process_threads`](#conduit_process_threads), [`conduit_process_cpu_seconds_total`](#conduit_process_cpu_seconds_total) | — | — | yes (`process` category / `standard`, Linux `/proc`) |

**Hot path** — incremented while handling queries on listener workers. **Scrape-time only** — refreshed when metrics are exported (Prometheus scrape or OTEL push), not on the hot path; see [Scrape-time gauges](#scrape-time-gauges). A dash (—) means the series is not updated in that mode.

Config schema: [Metrics and tracing](/reference/config-schema/metrics-and-tracing.md).

---

## Query path (by pipeline phase)

### conduit_queries_total { #conduit_queries_total }

| | |
|--|--|
| **Type** | Counter |
| **Labels (`minimal`)** | `listener`, `protocol` (`udp` / `tcp`) |
| **Labels (`full`)** | above + `qtype`, `qclass`, `ip_family` (`v4` / `v6`) |
| **When** | After a successful [Parse](/concepts/architecture-and-packet-path.md#parse), before [Request rules](/concepts/architecture-and-packet-path.md#request-rules) |
| **Not counted** | [Parse](/concepts/architecture-and-packet-path.md#parse) drops |

Policy [drops](#conduit_queries_dropped_total) still increment this counter (the query was parsed); they do **not** increment [`conduit_responses_total`](#conduit_responses_total).

### conduit_parse_rejected_total { #conduit_parse_rejected_total }


| | |
|--|--|
| **Type** | Counter |
| **Labels** | `reason` |
| **Profile** | `minimal` and `full` |
| **When** | [Parse](/concepts/architecture-and-packet-path.md#parse) returns **drop** |

`reason` values:

| `reason` | Meaning |
|----------|---------|
| `empty` | Zero-length packet |
| `wire_error` | Bytes are not a valid DNS message |
| `not_query` | Message is not a query (for example a response) |
| `no_question` | Query with no question section |
| `multi_question` | More than one question |

### conduit_acl_decisions_total { #conduit_acl_decisions_total }

| | |
|--|--|
| **Type** | Counter |
| **Labels (`minimal`)** | `tier`, `action`, `listener` |
| **Labels (`full`)** | above + `ip_family` (`v4` / `v6`) |
| **Profile** | `minimal` and `full` |
| **When** | Host [Client ACL](/policy-routing/client-acls.md) gate records a decision |

`tier` values:

| `tier` | Meaning |
|--------|---------|
| `preadmission` | Tier 0 — explicit **`drop`** before structural parse |
| `listener` | Tier 1 — full policy after parse, before slot acquire |

`action` values: `drop`, `refuse`, `tag`, `admit`.

Host ACL gates only — rule/`client_cidr` / Rhai policy does **not** increment this series (use rule drops or [user metrics](/rhai/user-metrics.md) instead). Same series on Prometheus scrape and OTLP push.

### conduit_queries_by_pool_total { #conduit_queries_by_pool_total }

| | |
|--|--|
| **Type** | Counter |
| **Labels** | `pool` |
| **When** | After [Route](/concepts/architecture-and-packet-path.md#route) selects a pool inside the forward provider and upstream send proceeds |

Includes each [retry](/glossary/index.md#retry) attempt whose forward provider reaches pool selection. **Not** incremented on cache hit short-circuit.

### conduit_phase_duration_seconds { #conduit_phase_duration_seconds }

| | |
|--|--|
| **Type** | Histogram |
| **Labels** | `phase` |
| **Profile** | `full` only (not incremented on `minimal`) |
| **When** | Each registered top-level [pipeline phase](/concepts/architecture-and-packet-path.md#pipeline-phases) stage completes |

`phase` values: `receive`, `parse`, `request_rules`, `lookup`, `response_rules`, `send`.

Route, forward, and wait-for-response run **inside** the forward lookup provider; they do **not** appear as separate top-level `phase` label values. Use nested trace events or [`conduit_lookup_duration_seconds`](#conduit_lookup_duration_seconds) for provider-level timing.

Bucket upper bounds (seconds, cumulative): 100 µs, 1 ms, 10 ms, 50 ms, 100 ms, 500 ms, 1 s, 5 s, 10 s. Use `histogram_quantile()` in PromQL for percentiles.

### conduit_forward_attempts_total { #conduit_forward_attempts_total }

| | |
|--|--|
| **Type** | Counter |
| **Labels** | `pool`, `backend`, `outcome` |
| **Profile** | `full` only (not incremented on `minimal`) |
| **When** | Each upstream forward attempt completes inside the forward lookup provider |

**Not** incremented when a cache provider answers without running forward.

`outcome`: `success` or `error`.

### conduit_forward_errors_total { #conduit_forward_errors_total }

| | |
|--|--|
| **Type** | Counter |
| **Labels** | `pool`, `backend`, `reason` |
| **Profile** | `minimal` and `full` |
| **When** | A forward attempt ends with `outcome="error"` |

`backend` is the configured backend `name` when set, otherwise its `address`. When no backend was selected (for example `reason="no_backend"`), `backend` is `unknown`.

Common `reason` values:

| `reason` | Typical cause |
|----------|----------------|
| `timeout` | No upstream reply before `forward.timeout_ms` |
| `send_error` | UDP send to backend failed |
| `tcp_error` | TCP forward failed |
| `table_full` | Too many in-flight forwards to the same backend (`forward.outstanding_per_backend`) |
| `no_backend` | No backend selected (should be rare if [Route](/concepts/architecture-and-packet-path.md#route) succeeded) |

### conduit_forward_duration_seconds { #conduit_forward_duration_seconds }

| | |
|--|--|
| **Type** | Histogram |
| **Labels** | `pool`, `backend` |
| **Profile** | `full` only (not incremented on `minimal`) |
| **When** | Each forward attempt completes (success or error) |

Bucket upper bounds (seconds): 1 ms, 10 ms, 50 ms, 100 ms, 500 ms, 1 s, 5 s, 10 s.

### conduit_retries_total { #conduit_retries_total }

| | |
|--|--|
| **Type** | Counter |
| **Labels** | `pool` |
| **Profile** | `minimal` and `full` |
| **When** | [Response rules](/concepts/architecture-and-packet-path.md#response-rules) send the pipeline back to [Lookup](/concepts/architecture-and-packet-path.md#lookup) for a [retry](/glossary/index.md#retry) |
| **Label `pool`** | Target [pool](/glossary/index.md#pool) for the next attempt (`retry_pool` from **`set_retry_pool`** when set, otherwise the current pool) |

### conduit_responses_total { #conduit_responses_total }

| | |
|--|--|
| **Type** | Counter |
| **Labels (`minimal`)** | `listener`, `protocol`, `rcode`, `answer_source` |
| **Labels (`full`)** | above + `ip_family` (`v4` / `v6`) |
| **When** | [Send](/concepts/architecture-and-packet-path.md#send) completes — upstream answer or synthesized error |

`answer_source` is `cache` or `forward` when the lookup spine produced the answer; empty when unknown (for example some synthesized errors).

Both profiles use the label name **`rcode`**, but bucketing differs:

| Profile | `rcode` values |
|---------|----------------|
| **`minimal`** | Coarse buckets: `NOERROR`, `NXDOMAIN`, `SERVFAIL`, `REFUSED`, `OTHER` (uncommon IANA codes roll into `OTHER`) |
| **`full`** | Per-IANA names for codes 0–23 (for example `FORMERR`, `NOTAUTH`, `BADCOOKIE`); unknown codes → `OTHER` |

**Breaking change (full profile):** the label is **`rcode`**, not **`rcode_class`**. PromQL or dashboards that group on **`rcode_class`** must use **`rcode`** instead; the full profile can split on individual IANA codes.

### conduit_responses_truncated_total { #conduit_responses_truncated_total }

| | |
|--|--|
| **Type** | Counter |
| **Labels (`minimal`)** | `listener`, `protocol`, `answer_source` |
| **Labels (`full`)** | above + `ip_family` (`v4` / `v6`) |
| **Profile** | `minimal` and `full` |
| **When** | [Send](/concepts/architecture-and-packet-path.md#send) fits outbound **UDP** responses to the client's payload size (EDNS bufsize or 512-byte default) on RR boundaries and sets the **TC** bit when required data cannot fit |

Incremented at most once per transaction, alongside [`conduit_responses_total`](#conduit_responses_total). Labels align so you can compare truncation rate by listener, protocol, and how the answer was produced (`cache` vs `forward`). Truncation is **egress** behavior — it can happen for cache hits when the stored wire answer exceeds the client's bufsize, not only on forward paths.

PromQL example (truncation share of responses):

```promql
sum(rate(conduit_responses_truncated_total[5m])) by (listener, answer_source)
  / sum(rate(conduit_responses_total[5m])) by (listener, answer_source)
```

Enable **`debug`** logging to see per-transaction truncation detail (`wire_len_before`, `wire_len_after`, `client_udp_payload_size`).

### conduit_queries_dropped_total { #conduit_queries_dropped_total }

| | |
|--|--|
| **Type** | Counter |
| **Labels (`minimal`)** | `listener`, `protocol`, `reason` |
| **Labels (`full`)** | above + `ip_family` (`v4` / `v6`) |
| **Profile** | `minimal` and `full` |
| **When** | [Request rules](/concepts/architecture-and-packet-path.md#request-rules) or [Response rules](/concepts/architecture-and-packet-path.md#response-rules) **drop** (built-in action or Rhai) ends the [transaction](/glossary/index.md#transaction) with **no DNS reply** |

`reason` values:

| `reason` | Meaning |
|----------|---------|
| `request_rules` | Policy drop on the request hook (before [Lookup](/concepts/architecture-and-packet-path.md#lookup)) |
| `response_rules` | Policy drop on the response hook (after an answer was produced, before [Send](/concepts/architecture-and-packet-path.md#send)) |

Parse-stage silent rejects use [`conduit_parse_rejected_total`](#conduit_parse_rejected_total), not this series. Upstream timeouts and other forward failures still produce a client reply (typically SERVFAIL) and appear on [`conduit_forward_errors_total`](#conduit_forward_errors_total) / [`conduit_responses_total`](#conduit_responses_total) — they are **not** policy drops.

Labels align with [`conduit_queries_total`](#conduit_queries_total) so you can compare drop rate by listener and protocol. For qname-level detail, use [event export](/observability/event-export.md) or [logs](/observability/logging.md); for custom categories (for example blocklist hits), use [Rhai user metrics](/rhai/user-metrics.md).

PromQL example (request-hook drop share of parsed queries):

```promql
sum(rate(conduit_queries_dropped_total{reason="request_rules"}[5m])) by (listener)
  / sum(rate(conduit_queries_total[5m])) by (listener)
```

### conduit_script_errors_total { #conduit_script_errors_total }

| | |
|--|--|
| **Type** | Counter |
| **Labels** | `reason`, `script`, `table` |
| **Profile** | `minimal` and `full` |
| **When** | Rhai hook evaluation faults: script errors, sandbox limits, phase guards, or unknown lookup table at runtime |

`reason` values:

| `reason` | Meaning |
|----------|---------|
| `lookup_unknown_table` | `lookup` used a table name not in **`data_sources:`** (`table` label is the sanitized name; dynamic unsafe names → `other`) |
| `phase_guard` | API called on the wrong hook (for example `set_source_v4` on response) |
| `timeout` | Hook exceeded `rhai.hook_timeout_ms` |
| `operation_limit` | Rhai operations budget exhausted |
| `eval` | Other script evaluation error |

`script` is the configured Rhai file path. `table` is empty except for `lookup_unknown_table`.

PromQL example:

```promql
sum(rate(conduit_script_errors_total[5m])) by (reason)
sum(rate(conduit_script_errors_total{reason="lookup_unknown_table"}[5m])) by (script, table)
```

See [Lookups — lookup behavior](/rhai/data-sources-and-lookups.md#lookup-behavior) for compile-time literal checks vs runtime unknown-table behavior.

---

## Lookup and cache { #lookup-and-cache }

Series for the [Lookup](/concepts/architecture-and-packet-path.md#lookup) phase and optional DNS answer cache. Guide: [DNS answer cache](/guides/dns-answer-cache.md).

### conduit_lookup_provider_outcomes_total { #conduit_lookup_provider_outcomes_total }

| | |
|--|--|
| **Type** | Counter |
| **Labels** | `profile`, `provider`, `outcome` |
| **Profile** | `minimal` and `full` |
| **When** | A lookup provider reaches a terminal outcome for an attempt |

`provider`: `cache` or `forward`. Common `outcome` values: `answered`, `miss`, `bypass`, `pending`.

### conduit_cache_lookups_total { #conduit_cache_lookups_total }

| | |
|--|--|
| **Type** | Counter |
| **Labels** | `cache`, `profile`, `result` |
| **Profile** | `minimal` and `full` |
| **When** | Cache provider read path |

`result`: `hit`, `miss`, or `bypass`.

### conduit_cache_fills_total { #conduit_cache_fills_total }

| | |
|--|--|
| **Type** | Counter |
| **Labels** | `cache`, `profile` |
| **Profile** | `full` only |
| **When** | Successful cache store after an upstream answer |

### conduit_cache_singleflight_coalesced_total { #conduit_cache_singleflight_coalesced_total }

| | |
|--|--|
| **Type** | Counter |
| **Labels** | `cache`, `profile` |
| **Profile** | `full` only |
| **When** | A parallel identical cache miss joins an in-progress fill and is answered when that fill completes |

On a cache miss, identical queries **single-flight**: one query fetches upstream and fills the cache; others wait and then take that shared answer instead of starting their own forward. This counter increments once per waiting query that is served that way — not for the query that performed the fill. See [DNS answer cache — Hit and miss path](/guides/dns-answer-cache.md#hit-and-miss-path).

### conduit_lookup_duration_seconds { #conduit_lookup_duration_seconds }

| | |
|--|--|
| **Type** | Histogram |
| **Labels** | `profile`, `provider` |
| **Profile** | `full` only |
| **When** | Wall time in one lookup provider attempt |

Same bucket layout as [`conduit_phase_duration_seconds`](#conduit_phase_duration_seconds).

### conduit_cache_lookup_duration_seconds { #conduit_cache_lookup_duration_seconds }

| | |
|--|--|
| **Type** | Histogram |
| **Labels** | `cache`, `profile` |
| **Profile** | `full` only |
| **When** | Cache read path latency |

### conduit_response_duration_seconds { #conduit_response_duration_seconds }

| | |
|--|--|
| **Type** | Histogram |
| **Labels** | `answer_source`, `listener`, `protocol` |
| **Profile** | `full` only |
| **When** | [Send](/concepts/architecture-and-packet-path.md#send) completes |

End-to-end client response time split by how the answer was produced (`cache` or `forward`).

PromQL examples (cache vs forward):

```promql
sum(rate(conduit_responses_total[5m])) by (listener, answer_source)
sum(rate(conduit_cache_lookups_total[5m])) by (cache, result)
histogram_quantile(0.99, sum(rate(conduit_lookup_duration_seconds_bucket[5m])) by (le, provider))
```

---

## Scrape-time gauges { #scrape-time-gauges }

Series marked **Scrape-time only** in the [profile table](#profiles) above. Values are refreshed when metrics are exported (Prometheus scrape or OTEL push), not incremented on listener workers during queries.

### conduit_forward_outstanding { #conduit_forward_outstanding }

| | |
|--|--|
| **Type** | Gauge |
| **Labels** | `pool`, `backend` |
| **Meaning** | In-flight upstream forwards per backend at scrape time |

A forward counts as outstanding from the moment it is submitted upstream until the reply arrives, times out, or errors. Under the **`split_io`** [runtime](/concepts/runtime-and-concurrency.md#runtime-models) this includes **parked** waits — transactions suspended in the [Wait for response](/concepts/architecture-and-packet-path.md#wait-for-response) phase whose [slot](/concepts/runtime-and-concurrency.md#transaction-slot-pool) is held while the policy worker moves on to other queries. A sustained high value against a slow upstream is the expected `split_io` signal (concurrent waits), not a backlog of busy workers as it would be under `sync`.

### conduit_slots_in_use { #conduit_slots_in_use }

| | |
|--|--|
| **Type** | Gauge |
| **Labels** | — |
| **Profile** | `full` / `standard` only (`runtime` category) |
| **Meaning** | [Transaction slots](/concepts/runtime-and-concurrency.md#transaction-slot-pool) currently acquired (not `Free`) at scrape time |

Every in-flight transaction holds one slot for its whole lifetime, including while parked in [Wait for response](/concepts/architecture-and-packet-path.md#wait-for-response) under `split_io`. Compare against [`conduit_slots_capacity`](#conduit_slots_capacity) to gauge headroom; sustained `in_use` near `capacity` precedes slot-pool exhaustion.

### conduit_slots_capacity { #conduit_slots_capacity }

| | |
|--|--|
| **Type** | Gauge |
| **Labels** | — |
| **Profile** | `full` / `standard` only (`runtime` category) |
| **Meaning** | Configured transaction slot-pool capacity (`orchestrator.txn_table_capacity`) at scrape time |

### conduit_slot_pool_exhausted_total { #conduit_slot_pool_exhausted_total }

| | |
|--|--|
| **Type** | Counter |
| **Labels** | — |
| **Profile** | `minimal` and `full` |
| **Meaning** | Cumulative slot-acquire failures because the pool was at capacity |

Each increment is a query that could not get a [slot](/concepts/runtime-and-concurrency.md#transaction-slot-pool) and was shed (backpressure). The cumulative total is synced into the export registry at scrape time. A non-zero rate means the slot pool is the bottleneck — raise `orchestrator.txn_table_capacity` (a **restart** is required to grow the pool) or reduce inbound load. See [Per-pool in-flight limit](/reference/config-schema/pools.md#per-pool-in-flight-limit) for a pool-scoped cap that returns SERVFAIL instead.

### conduit_pool_backends_configured { #conduit_pool_backends_configured }

| | |
|--|--|
| **Type** | Gauge |
| **Labels** | `pool` |
| **Meaning** | Number of backends configured in each pool (active snapshot) |

### conduit_config_generation { #conduit_config_generation }

| | |
|--|--|
| **Type** | Gauge |
| **Labels** | — |
| **Meaning** | Active [runtime snapshot](/glossary/index.md#runtime-snapshot) generation |

Useful after reload/apply to confirm which config generation is live. See [Runtime snapshot](/concepts/architecture-and-packet-path.md#runtime-snapshot).

### conduit_pool_backends_active { #conduit_pool_backends_active }

| | |
|--|--|
| **Type** | Gauge |
| **Labels** | `pool` |
| **Profile** | `minimal` and `full` (`health` category) |
| **Meaning** | Count of backends in the pool with **applied** health **up** (eligible for Route) at scrape time |

Only emitted for pools with health checking enabled. Compare to [`conduit_pool_backends_configured`](#conduit_pool_backends_configured) for how many backends are down or unknown.

---

## Backend health { #backend-health }

Scrape-time gauges for [backend health](/policy-routing/backend-health.md). Exported when the **`health`** category is in the active plan (included in **`base: minimal`** and **`standard`**) and at least one pool has health enabled. Labels are **`pool`** and **`backend`** only — the `backend` label is the configured `name` when set, otherwise `address`. No per-qname or per-client dimensions.

Liveness encoding for observed/applied gauges:

| Value | Meaning |
|-------|---------|
| `0` | unknown |
| `1` | up |
| `2` | down |

### conduit_backend_health_observed { #conduit_backend_health_observed }

| | |
|--|--|
| **Type** | Gauge |
| **Labels** | `pool`, `backend` |
| **Profile** | `minimal` and `full` (`health` category) |
| **Meaning** | Health from probes and passive fast-trip (what Conduit observes before operator overrides) |

### conduit_backend_health_applied { #conduit_backend_health_applied }

| | |
|--|--|
| **Type** | Gauge |
| **Labels** | `pool`, `backend` |
| **Profile** | `minimal` and `full` (`health` category) |
| **Meaning** | Health [Route](/concepts/architecture-and-packet-path.md#route) uses for eligibility |

May differ from [`conduit_backend_health_observed`](#conduit_backend_health_observed) when a backend is [frozen](/glossary/index.md#freeze) or [drained](/glossary/index.md#drain).

### conduit_backend_health_probe_automatic { #conduit_backend_health_probe_automatic }

| | |
|--|--|
| **Type** | Gauge |
| **Labels** | `pool`, `backend` |
| **Profile** | `minimal` and `full` (`health` category) |
| **Meaning** | `1` = probe-driven transitions apply; `0` = [frozen](/glossary/index.md#freeze) at this backend's resolved scope |

### conduit_backend_health_effective_weight { #conduit_backend_health_effective_weight }

| | |
|--|--|
| **Type** | Gauge |
| **Labels** | `pool`, `backend` |
| **Profile** | `minimal` and `full` (`health` category) |
| **Meaning** | Effective load-balancing weight Route uses (0 when applied down) |

### conduit_backend_health_latency_ewma_ms { #conduit_backend_health_latency_ewma_ms }

| | |
|--|--|
| **Type** | Gauge |
| **Labels** | `pool`, `backend` |
| **Profile** | `minimal` and `full` (`health` category) |
| **Meaning** | Probe round-trip EWMA in milliseconds |

### conduit_backend_health_transitions_total { #conduit_backend_health_transitions_total }

| | |
|--|--|
| **Type** | Counter |
| **Labels** | `pool`, `backend` |
| **Profile** | `minimal` and `full` (`health` category) |
| **Meaning** | Cumulative observed or applied health transitions |

### conduit_probe_results_total { #conduit_probe_results_total }

| | |
|--|--|
| **Type** | Counter |
| **Labels** | `pool`, `backend`, `outcome` |
| **Profile** | `minimal` and `full` (`health` category) |
| **When** | Each active health probe completes (success, failure, timeout, or send error) |

`outcome` values:

| `outcome` | Meaning |
|-----------|---------|
| `success` | Probe reply accepted as healthy |
| `failure` | Probe reply received but not acceptable (for example narrowed `acceptable_rcodes`) |
| `timeout` | No reply before the probe timeout |
| `send_error` | Probe could not be sent (UDP send failure or TCP connect/transport failure) |

Unmatched replies (wrong query id or garbage) do not increment this counter; the outstanding probe stays open until a matching reply or timeout.

PromQL examples:

```promql
conduit_pool_backends_active / conduit_pool_backends_configured
conduit_backend_health_applied{pool="default"} == 2
conduit_backend_health_observed != conduit_backend_health_applied
sum(rate(conduit_probe_results_total{outcome!="success"}[5m])) by (pool, backend, outcome)
```

Process logs emit health transitions when `observed` or `applied` changes. Active-probe transitions log `backend health transition` at INFO. Passive fast-trip logs each counting failure at WARN (`passive health: forward failure`) and the threshold-crossing event as `passive fast-trip: backend marked down` (with pool, backend, reason, qname, qtype, and client).

---

## Process and build

### conduit_build_info { #conduit_build_info }

| | |
|--|--|
| **Type** | Gauge (value `1`) |
| **Labels** | `version`, `revision`, `dirty`, `profile` |
| **When** | Set at **compile time** (not runtime) |

| Label | Meaning |
|-------|---------|
| `version` | Workspace semver from `Cargo.toml` |
| `revision` | Short git commit, or `unknown` outside a git checkout |
| `dirty` | `true` if the tree had uncommitted changes at build time |
| `profile` | Cargo profile (`debug` or `release`) |

Rebuild after pulling or editing sources so `revision` and `dirty` match the binary you run.

### conduit_start_time_seconds { #conduit_start_time_seconds }

| | |
|--|--|
| **Type** | Gauge |
| **Labels** | — |
| **Category** | `meta` |
| **Meaning** | Unix timestamp when the process started (wall clock) |

Useful for correlating restarts with absolute time. Prefer [`conduit_uptime_seconds`](#conduit_uptime_seconds) for how long the process has been running — wall-clock jumps (NTP, VM pause/resume) can make `time() - conduit_start_time_seconds` wrong.

### conduit_uptime_seconds { #conduit_uptime_seconds }

| | |
|--|--|
| **Type** | Gauge |
| **Labels** | — |
| **Category** | `meta` |
| **Meaning** | Seconds since process start, from a monotonic clock |

Refreshed on each Prometheus scrape or OTLP push gather. Immune to wall-clock skew. Survives metrics plan hot-swaps within the same process.

PromQL example: `conduit_uptime_seconds` (alert when unexpectedly low after a restart, or graph directly).

### conduit_process_resident_bytes { #conduit_process_resident_bytes }

| | |
|--|--|
| **Type** | Gauge |
| **Category** | `process` (`standard`; not on `minimal` unless included) |
| **Platform** | Linux (`/proc/self/status`) |
| **Meaning** | Process resident set size in bytes |

### conduit_process_open_fds { #conduit_process_open_fds }

| | |
|--|--|
| **Type** | Gauge |
| **Category** | `process` (`standard`; not on `minimal` unless included) |
| **Platform** | Linux (`/proc/self/fd`) |
| **Meaning** | Open file descriptors |

### conduit_process_max_fds { #conduit_process_max_fds }

| | |
|--|--|
| **Type** | Gauge |
| **Category** | `process` (`standard`; not on `minimal` unless included) |
| **Platform** | Linux (`/proc/self/limits`) |
| **Meaning** | Soft limit on open file descriptors |

Use with [`conduit_process_open_fds`](#conduit_process_open_fds) for headroom alerts (`open_fds / max_fds`). When the soft limit is `unlimited`, the gauge is `0`.

PromQL example: `conduit_process_open_fds / conduit_process_max_fds`.

### conduit_process_threads { #conduit_process_threads }

| | |
|--|--|
| **Type** | Gauge |
| **Category** | `process` (`standard`; not on `minimal` unless included) |
| **Platform** | Linux (`/proc/self/status`) |
| **Meaning** | Number of threads in this process |

### conduit_process_cpu_seconds_total { #conduit_process_cpu_seconds_total }

| | |
|--|--|
| **Type** | Counter |
| **Category** | `process` (`standard`; not on `minimal` unless included) |
| **Platform** | Linux (`/proc/self/stat`) |
| **Meaning** | Cumulative user + system CPU time for this process, in seconds |

PromQL example: `rate(conduit_process_cpu_seconds_total[5m])` for CPU cores used.

---

## Event export

Per-sink counters are included at scrape time from `EventHub` snapshots (not incremented on workers). See [Event export](/observability/event-export.md).

### conduit_events_enqueued_query_total { #conduit_events_enqueued_query_total }

| | |
|--|--|
| **Type** | Counter |
| **Labels** | `sink` |
| **When** | Query-phase export events enqueued |

### conduit_events_enqueued_response_total { #conduit_events_enqueued_response_total }

| | |
|--|--|
| **Type** | Counter |
| **Labels** | `sink` |
| **When** | Response-phase export events enqueued |

### conduit_events_queue_dropped_total { #conduit_events_queue_dropped_total }

| | |
|--|--|
| **Type** | Counter |
| **Labels** | `sink` |
| **When** | Export queue overflow ([Runtime and concurrency](/concepts/runtime-and-concurrency.md#query-outcomes-and-worker-occupancy)) |

### conduit_events_delivered_total { #conduit_events_delivered_total }

| | |
|--|--|
| **Type** | Counter |
| **Labels** | `sink` |
| **When** | Frames successfully delivered to a sink |

---

## User metrics (Rhai)

Scripts can call `metric_inc` / `metric_inc_labels` from [Rhai](/rhai/index.md) hooks. Series are prefixed `conduit_user_<name>` and flush after each successful hook when that metric's **collect** mask is on. **Emit** controls whether the series appears in Prometheus / OTLP. Optional **`help`** on `metrics.user_metrics[]` sets scrape `# HELP` and the OTel description. On **`base: minimal`**, unlisted script metrics stay off (increments no-op with a warning) until you opt them in — see [User metrics — Collect and emit](/rhai/user-metrics.md#export-tier).

---

## PromQL examples

```promql
sum(rate(conduit_queries_total[5m])) by (listener, protocol)
sum(rate(conduit_queries_by_pool_total[5m])) by (pool)
sum(rate(conduit_queries_dropped_total[5m])) by (reason)
sum(rate(conduit_parse_rejected_total[5m])) by (reason)
sum(rate(conduit_responses_total[5m])) by (rcode)
sum(rate(conduit_responses_total[5m])) by (rcode, ip_family)   # fine / base: standard labels
sum(rate(conduit_responses_truncated_total[5m])) by (listener, answer_source)
sum(rate(conduit_responses_total[5m])) by (listener, answer_source)
sum(rate(conduit_lookup_provider_outcomes_total[5m])) by (profile, provider, outcome)
sum(rate(conduit_cache_lookups_total[5m])) by (cache, result)
histogram_quantile(0.99, sum(rate(conduit_lookup_duration_seconds_bucket[5m])) by (le, provider))
sum(rate(conduit_forward_errors_total[5m])) by (pool, backend, reason)
sum(rate(conduit_script_errors_total[5m])) by (reason)
histogram_quantile(0.99, sum(rate(conduit_forward_duration_seconds_bucket[5m])) by (le, pool))
sum(conduit_forward_outstanding) by (pool, backend)                 # concurrent upstream waits (split_io)
conduit_slots_in_use / conduit_slots_capacity                       # slot-pool utilization (base: standard)
sum(rate(conduit_slot_pool_exhausted_total[5m]))                    # slot exhaustion (alert on > 0)
conduit_config_generation
conduit_uptime_seconds
conduit_pool_backends_active
conduit_backend_health_applied{pool="default"}
conduit_build_info{revision="abc1234", dirty="false", profile="release"}
```

Prometheus scrape and OTEL push both consume the same metric families. Scrape exposes the Prometheus text form (including histogram `_bucket` / `_sum` / `_count` series). OTLP push maps each family to an equivalent OTLP instrument — same names and label sets, HELP as description, units derived from name suffixes, and histograms with matching sum, count, and explicit bucket counts.
