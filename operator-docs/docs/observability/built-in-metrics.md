# Built-in metrics

Catalog of **built-in** Prometheus series exported by Conduit (not Rhai `conduit_user_*` metrics). For enabling scrape, profiles, and OTEL push, see [Metrics](/observability/metrics.md). For how metrics map to the query path, see [Architecture and packet path](/concepts/architecture-and-packet-path.md).

Built-in labels never include `qname`, client IP, or transaction id — use [event export](/observability/event-export.md) or [tracing](/observability/tracing.md) for per-name detail.

## Profiles { #profiles }

### Enabling export

When the `metrics` section is **omitted** from config, export is disabled — no scrape listener, no hot-path increments, no built-in series.

To enable built-ins, add a `metrics:` block with `enabled: true` and configure at least one export path (Prometheus scrape and/or OTEL push). Example:

```yaml
metrics:
  enabled: true
  profile: full          # or minimal
  prometheus:
    listen_address: "127.0.0.1:9090"
    path: /metrics
```

| Setting {: .column-no-wrap } | Meaning |
|---------|---------|
| `metrics.enabled` | Must be `true` for any built-in export or hot-path recording |
| `metrics.profile` | `minimal` or `full` (default **`full`** when the block is present). `off` disables export even if `enabled` is true |
| `metrics.prometheus` | Optional HTTP scrape listener (`listen_address`, `path`) |
| `metrics.otel` | Optional OTLP HTTP push (`endpoint`, `push_interval_ms`) |

`minimal` and `full` control **what** Conduit records, not **how** you export it. Prometheus scrape, OTEL push, and both together all expose the same built-in series for the profile you chose. See [Metrics](/observability/metrics.md) and [Operator metrics profiles](/guides/operator-metrics-profiles.md).

### `minimal` vs `full` (hot path)

Both profiles record metrics **while handling queries** on listener workers (the **hot path**). The difference is **how much** is recorded and **how many label dimensions** are kept — a cardinality and overhead trade-off.

**`minimal`** — low-cardinality volume counters only:

- [`conduit_queries_total`](#conduit_queries_total) with `listener` and `protocol` only
- [`conduit_queries_by_pool_total`](#conduit_queries_by_pool_total) per `pool`

Use **`minimal`** when you want query volume and pool mix without per-qtype detail, parse-failure breakdown, response codes, forward latency, or per-phase histograms on the hot path.

**`full`** — complete built-in observability on the hot path:

- Richer [`conduit_queries_total`](#conduit_queries_total) labels (`qtype`, `qclass`, `ip_family`)
- Parse failures, client responses, phase timings, forward attempts/errors/RTT, and retries (see table below)
- Linux process gauges ([`conduit_process_resident_bytes`](#conduit_process_resident_bytes), [`conduit_process_open_fds`](#conduit_process_open_fds)) at scrape time

Use **`full`** for day-two operations, SLO dashboards, and debugging upstream or pipeline behavior. Built-in labels still never include `qname`, client IP, or transaction id at either profile.

**Scrape-time** series ([Scrape-time gauges](#scrape-time-gauges)) are largely the same for both profiles; only the process gauges require **`full`**.

| Series | Hot path `minimal` | Hot path `full` | [Scrape-time](#scrape-time-gauges) only |
|--------|-------------------|-----------------|-------------|
| [`conduit_queries_total`](#conduit_queries_total) | `listener`, `protocol` | + `qtype`, `qclass`, `ip_family` | — |
| [`conduit_queries_by_pool_total`](#conduit_queries_by_pool_total) | yes (`pool`) | yes | — |
| [`conduit_parse_rejected_total`](#conduit_parse_rejected_total) | no | yes (`reason`) | — |
| [`conduit_responses_total`](#conduit_responses_total) | yes (`listener`, `protocol`, coarse `rcode`) | yes (+ fine `rcode`, `ip_family`) | — |
| Phase / forward / retry histograms & counters below | no | yes | — |
| [`conduit_forward_outstanding`](#conduit_forward_outstanding) | — | — | yes |
| [`conduit_pool_backends_configured`](#conduit_pool_backends_configured) | — | — | yes |
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
| **Profile** | `full` only |
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
| **Profile** | `full` only |
| **When** | Each upstream forward attempt completes ([Forward](/concepts/architecture-and-packet-path.md#forward) / [Wait for response](/concepts/architecture-and-packet-path.md#wait-for-response)) |

`outcome`: `success` or `error`.

### conduit_forward_errors_total { #conduit_forward_errors_total }

| | |
|--|--|
| **Type** | Counter |
| **Labels** | `pool`, `reason` |
| **Profile** | `full` only |
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
| **Profile** | `full` only |
| **When** | Each forward attempt completes (success or error) |

Bucket upper bounds (seconds): 1 ms, 10 ms, 50 ms, 100 ms, 500 ms, 1 s, 5 s, 10 s.

### conduit_retries_total { #conduit_retries_total }

| | |
|--|--|
| **Type** | Counter |
| **Labels** | `pool` |
| **Profile** | `full` only |
| **When** | [Response rules](/concepts/architecture-and-packet-path.md#response-rules) send the pipeline back to [Route](/concepts/architecture-and-packet-path.md#route) for a [retry](/glossary/index.md#retry) |
| **Label `pool`** | Target [pool](/glossary/index.md#pool) for the next attempt (`retry_pool` override when set, otherwise the current pool) |

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

---

## Scrape-time gauges { #scrape-time-gauges }

Series marked **Scrape-time only** in the [profile table](#profiles) above. Values are refreshed when metrics are exported (Prometheus scrape or OTEL push), not incremented on listener workers during queries.

### conduit_forward_outstanding { #conduit_forward_outstanding }

| | |
|--|--|
| **Type** | Gauge |
| **Labels** | `pool`, `backend` |
| **Meaning** | In-flight upstream forwards per backend at scrape time |

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
| **When** | Export queue overflow ([Concurrency and workers](/concepts/architecture-and-packet-path.md#concurrency-and-workers)) |

### conduit_events_delivered_total { #conduit_events_delivered_total }

| | |
|--|--|
| **Type** | Counter |
| **Labels** | `sink` |
| **When** | Frames successfully delivered to a sink |

---

## User metrics (Rhai)

Scripts can call `metric_inc` / `metric_inc_labels` from [Rhai](/rhai/index.md) hooks. Series are prefixed `conduit_user_<name>` and flushed into the export registry after each successful hook. Details: [User metrics](/rhai/user-metrics.md).

---

## PromQL examples

```promql
sum(rate(conduit_queries_total[5m])) by (listener, protocol)
sum(rate(conduit_queries_by_pool_total[5m])) by (pool)
sum(rate(conduit_parse_rejected_total[5m])) by (reason)
sum(rate(conduit_responses_total[5m])) by (rcode)
sum(rate(conduit_responses_total[5m])) by (rcode, ip_family)   # full profile only
sum(rate(conduit_forward_errors_total[5m])) by (pool, reason)
histogram_quantile(0.99, sum(rate(conduit_forward_duration_seconds_bucket[5m])) by (le, pool))
conduit_config_generation
conduit_build_info{revision="abc1234", dirty="false", profile="release"}
```

Prometheus scrape and OTEL push both consume the same metric families from `render_prometheus()`. Histogram `_bucket` series are available on Prometheus scrape; OTLP carries counter, gauge, and histogram summaries for built-ins.
