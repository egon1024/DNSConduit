# Built-in metrics

Catalog of **built-in** Prometheus series exported by Conduit (not Rhai `conduit_user_*` metrics). For enabling scrape, profiles, and OTEL push, see [Metrics](/observability/metrics.md). For how metrics map to the query path, see [Architecture and packet path](/concepts/architecture-and-packet-path.md).

Built-in labels never include `qname`, client IP, or transaction id — use [event export](/observability/event-export.md) or [tracing](/observability/tracing.md) for per-name detail.

The **`backend`** label on forward metrics ([`conduit_forward_attempts_total`](#conduit_forward_attempts_total), [`conduit_forward_duration_seconds`](#conduit_forward_duration_seconds), [`conduit_forward_outstanding`](#conduit_forward_outstanding)) is the backend [`name`](/policy-routing/pools-and-backends.md#backend-names) when one is set, otherwise its `address`. Naming a backend keeps its time series stable across an address change; see [Backend names](/policy-routing/pools-and-backends.md#backend-names).

## Profiles { #profiles }

Built-in recording uses a **`metrics.profile`** of **`minimal`** or **`full`** (see [Metrics](/observability/metrics.md) for enabling export, Prometheus scrape, OTEL push, and restart semantics). When the `metrics:` block is omitted, built-ins are off — no hot-path increments and no export.

`minimal` and `full` control **what** Conduit records on the hot path, not **how** you export it. Prometheus scrape, OTEL push, and both together expose the same built-in series for the profile you chose.

### `minimal` vs `full` (hot path)

Both profiles record metrics **while handling queries** on listener workers (the **hot path**). The difference is **how much** is recorded and **how many label dimensions** are kept — a cardinality and overhead trade-off.

**`minimal`** — low-cardinality volume and essential failure counters:

- [`conduit_queries_total`](#conduit_queries_total) with `listener` and `protocol` only
- [`conduit_queries_by_pool_total`](#conduit_queries_by_pool_total) per `pool`
- [`conduit_responses_total`](#conduit_responses_total) with coarse `rcode` buckets
- Failure counters: [`conduit_parse_rejected_total`](#conduit_parse_rejected_total), [`conduit_forward_errors_total`](#conduit_forward_errors_total), [`conduit_retries_total`](#conduit_retries_total), [`conduit_script_errors_total`](#conduit_script_errors_total)

Use **`minimal`** when you want query volume, pool mix, response mix, and alertable failure signals without per-qtype detail, forward latency histograms, or per-phase timing on the hot path.

**`full`** — complete built-in observability on the hot path:

- Richer [`conduit_queries_total`](#conduit_queries_total) labels (`qtype`, `qclass`, `ip_family`)
- Fine [`conduit_responses_total`](#conduit_responses_total) `rcode` labels and `ip_family`
- Forward attempt counts, forward RTT histograms, per-phase timing histograms (see table below)
- Transaction slot-pool gauges ([`conduit_slots_in_use`](#conduit_slots_in_use), [`conduit_slots_capacity`](#conduit_slots_capacity)) at scrape time
- Linux process gauges ([`conduit_process_resident_bytes`](#conduit_process_resident_bytes), [`conduit_process_open_fds`](#conduit_process_open_fds)) at scrape time

Use **`full`** for day-two operations, SLO dashboards, and debugging upstream or pipeline behavior. Built-in labels still never include `qname`, client IP, or transaction id at either profile.

**Scrape-time** series ([Scrape-time gauges](#scrape-time-gauges)) are largely the same for both profiles; the slot-pool gauges ([`conduit_slots_in_use`](#conduit_slots_in_use), [`conduit_slots_capacity`](#conduit_slots_capacity)) and the process gauges require **`full`**. The slot exhaustion counter ([`conduit_slot_pool_exhausted_total`](#conduit_slot_pool_exhausted_total)) is exported on both profiles.

| Series | Hot path `minimal` | Hot path `full` | [Scrape-time](#scrape-time-gauges) only |
|--------|-------------------|-----------------|-------------|
| [`conduit_queries_total`](#conduit_queries_total) | `listener`, `protocol` | + `qtype`, `qclass`, `ip_family` | — |
| [`conduit_queries_by_pool_total`](#conduit_queries_by_pool_total) | yes (`pool`) | yes | — |
| [`conduit_parse_rejected_total`](#conduit_parse_rejected_total) | yes (`reason`) | yes | — |
| [`conduit_responses_total`](#conduit_responses_total) | yes (`listener`, `protocol`, coarse `rcode`) | yes (+ fine `rcode`, `ip_family`) | — |
| [`conduit_forward_errors_total`](#conduit_forward_errors_total) | yes (`pool`, `reason`) | yes | — |
| [`conduit_retries_total`](#conduit_retries_total) | yes (`pool`) | yes | — |
| [`conduit_script_errors_total`](#conduit_script_errors_total) | yes (`reason`, `script`, `table`) | yes | — |
| Phase / forward-attempt / forward-duration histograms below | no | yes | — |
| [`conduit_forward_outstanding`](#conduit_forward_outstanding) | — | — | yes |
| [`conduit_pool_backends_configured`](#conduit_pool_backends_configured) | — | — | yes |
| [`conduit_slots_in_use`](#conduit_slots_in_use), [`conduit_slots_capacity`](#conduit_slots_capacity) | — | — | yes (`full` only) |
| [`conduit_slot_pool_exhausted_total`](#conduit_slot_pool_exhausted_total) | — | — | yes |
| [`conduit_build_info`](#conduit_build_info), [`conduit_start_time_seconds`](#conduit_start_time_seconds), [`conduit_config_generation`](#conduit_config_generation) | — | — | yes |
| [`conduit_process_resident_bytes`](#conduit_process_resident_bytes), [`conduit_process_open_fds`](#conduit_process_open_fds) | — | — | yes (`full` only, Linux `/proc`) |

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
| **Not counted** | [Parse](/concepts/architecture-and-packet-path.md#parse) drops; [policy drops](#policy-drops-no-built-in-counter) |

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

### conduit_queries_by_pool_total { #conduit_queries_by_pool_total }

| | |
|--|--|
| **Type** | Counter |
| **Labels** | `pool` |
| **When** | After [Route](/concepts/architecture-and-packet-path.md#route) selects a pool and the pipeline continues to [Forward](/concepts/architecture-and-packet-path.md#forward) |

Includes each [retry](/glossary/index.md#retry) attempt that reaches [Route](/concepts/architecture-and-packet-path.md#route) → [Forward](/concepts/architecture-and-packet-path.md#forward).

### conduit_phase_duration_seconds { #conduit_phase_duration_seconds }

| | |
|--|--|
| **Type** | Histogram |
| **Labels** | `phase` |
| **Profile** | `full` only (not incremented on `minimal`) |
| **When** | Each registered [pipeline phase](/concepts/architecture-and-packet-path.md#pipeline-phases) stage completes |

`phase` values: `receive`, `parse`, `request_rules`, `route`, `forward`, `wait_response`, `response_rules`, `send`.

Bucket upper bounds (seconds, cumulative): 100 µs, 1 ms, 10 ms, 50 ms, 100 ms, 500 ms, 1 s, 5 s, 10 s. Use `histogram_quantile()` in PromQL for percentiles.

### conduit_forward_attempts_total { #conduit_forward_attempts_total }

| | |
|--|--|
| **Type** | Counter |
| **Labels** | `pool`, `backend`, `outcome` |
| **Profile** | `full` only (not incremented on `minimal`) |
| **When** | Each upstream forward attempt completes ([Forward](/concepts/architecture-and-packet-path.md#forward) / [Wait for response](/concepts/architecture-and-packet-path.md#wait-for-response)) |

`outcome`: `success` or `error`.

### conduit_forward_errors_total { #conduit_forward_errors_total }

| | |
|--|--|
| **Type** | Counter |
| **Labels** | `pool`, `reason` |
| **Profile** | `minimal` and `full` |
| **When** | A forward attempt ends with `outcome="error"` |

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
| **When** | [Response rules](/concepts/architecture-and-packet-path.md#response-rules) send the pipeline back to [Route](/concepts/architecture-and-packet-path.md#route) for a [retry](/glossary/index.md#retry) |
| **Label `pool`** | Target [pool](/glossary/index.md#pool) for the next attempt (`retry_pool` from **`set_retry_pool`** when set, otherwise the current pool) |

### conduit_responses_total { #conduit_responses_total }

| | |
|--|--|
| **Type** | Counter |
| **Labels (`minimal`)** | `listener`, `protocol`, `rcode` |
| **Labels (`full`)** | above + `ip_family` (`v4` / `v6`) |
| **When** | [Send](/concepts/architecture-and-packet-path.md#send) completes — upstream answer or synthesized error |

Both profiles use the label name **`rcode`**, but bucketing differs:

| Profile | `rcode` values |
|---------|----------------|
| **`minimal`** | Coarse buckets: `NOERROR`, `NXDOMAIN`, `SERVFAIL`, `REFUSED`, `OTHER` (uncommon IANA codes roll into `OTHER`) |
| **`full`** | Per-IANA names for codes 0–23 (for example `FORMERR`, `NOTAUTH`, `BADCOOKIE`); unknown codes → `OTHER` |

**Breaking change (full profile):** earlier releases used the label name `rcode_class` with coarse buckets only. PromQL that grouped on `rcode_class` must use `rcode` instead; full-profile dashboards can now split on individual IANA codes.

### Policy drops (no built-in counter) { #policy-drops-no-built-in-counter }

[Request rules](/concepts/architecture-and-packet-path.md#request-rules) or [Response rules](/concepts/architecture-and-packet-path.md#response-rules) **drop** actions, and Rhai **drop** on those hooks, end the [transaction](/glossary/index.md#transaction) with **no DNS reply**. There is no dedicated drop counter; use [event export](/observability/event-export.md) or logs if you need visibility.

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

See [Data sources and lookups — lookup behavior](/rhai/data-sources-and-lookups.md#lookup-behavior) for compile-time literal checks vs runtime unknown-table behavior.

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
| **Profile** | `full` only |
| **Meaning** | [Transaction slots](/concepts/runtime-and-concurrency.md#transaction-slot-pool) currently acquired (not `Free`) at scrape time |

Every in-flight transaction holds one slot for its whole lifetime, including while parked in [Wait for response](/concepts/architecture-and-packet-path.md#wait-for-response) under `split_io`. Compare against [`conduit_slots_capacity`](#conduit_slots_capacity) to gauge headroom; sustained `in_use` near `capacity` precedes slot-pool exhaustion.

### conduit_slots_capacity { #conduit_slots_capacity }

| | |
|--|--|
| **Type** | Gauge |
| **Labels** | — |
| **Profile** | `full` only |
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
| **Meaning** | Unix timestamp when the process started |

PromQL example: `time() - conduit_start_time_seconds` for uptime.

### conduit_process_resident_bytes { #conduit_process_resident_bytes }

| | |
|--|--|
| **Type** | Gauge |
| **Profile** | `full` only |
| **Platform** | Linux (`/proc`) |
| **Meaning** | Process resident set size in bytes |

### conduit_process_open_fds { #conduit_process_open_fds }

| | |
|--|--|
| **Type** | Gauge |
| **Profile** | `full` only |
| **Platform** | Linux (`/proc`) |
| **Meaning** | Open file descriptors |

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

Scripts can call `metric_inc` / `metric_inc_labels` from [Rhai](/rhai/index.md) hooks. Series are prefixed `conduit_user_<name>` and flushed into the export registry after each successful hook when the metric's [export tier](/rhai/user-metrics.md#export-tier) matches the active `metrics.profile`. Details: [User metrics](/rhai/user-metrics.md).

---

## PromQL examples

```promql
sum(rate(conduit_queries_total[5m])) by (listener, protocol)
sum(rate(conduit_queries_by_pool_total[5m])) by (pool)
sum(rate(conduit_parse_rejected_total[5m])) by (reason)
sum(rate(conduit_responses_total[5m])) by (rcode)
sum(rate(conduit_responses_total[5m])) by (rcode, ip_family)   # full profile only
sum(rate(conduit_forward_errors_total[5m])) by (pool, reason)
sum(rate(conduit_script_errors_total[5m])) by (reason)
histogram_quantile(0.99, sum(rate(conduit_forward_duration_seconds_bucket[5m])) by (le, pool))
sum(conduit_forward_outstanding) by (pool, backend)                 # concurrent upstream waits (split_io)
conduit_slots_in_use / conduit_slots_capacity                       # slot-pool utilization (full profile)
sum(rate(conduit_slot_pool_exhausted_total[5m]))                    # slot exhaustion (alert on > 0)
conduit_config_generation
conduit_build_info{revision="abc1234", dirty="false", profile="release"}
```

Prometheus scrape and OTEL push both consume the same metric families from `render_prometheus()`. Histogram `_bucket` series are available on Prometheus scrape; OTLP carries counter, gauge, and histogram summaries for built-ins.
