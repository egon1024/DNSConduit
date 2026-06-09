# Metrics catalog

Reference for **built-in** Prometheus series exported by Conduit. For enabling scrape, profiles, and OTEL push, see [Metrics](/observability/metrics.md) and [Operator metrics profiles](/guides/operator-metrics-profiles.md). For how metrics map to the query path, see [Architecture and packet path](/concepts/architecture-and-packet-path.md).

Built-in labels never include `qname`, client IP, or transaction id — use [event export](/observability/event-export.md) or [tracing](/observability/tracing.md) for per-name detail.

## Profiles

When the `metrics` section is **omitted** from config, export is disabled (no scrape listener, no hot-path increments).

| Series | Hot path `minimal` | Hot path `full` | Scrape only |
|--------|-------------------|-----------------|-------------|
| [`conduit_queries_total`](#conduit_queries_total) | `listener`, `protocol` | + `qtype`, `qclass`, `ip_family` | — |
| [`conduit_queries_by_pool_total`](#conduit_queries_by_pool_total) | yes (`pool`) | yes | — |
| [`conduit_parse_rejected_total`](#conduit_parse_rejected_total) | no | yes (`reason`) | — |
| [`conduit_responses_total`](#conduit_responses_total) | no | yes | — |
| Phase / forward / retry histograms & counters below | no | yes | — |
| [`conduit_forward_outstanding`](#conduit_forward_outstanding) | — | — | yes |
| [`conduit_pool_backends_configured`](#conduit_pool_backends_configured) | — | — | yes |
| [`conduit_build_info`](#conduit_build_info), [`conduit_start_time_seconds`](#conduit_start_time_seconds), [`conduit_config_generation`](#conduit_config_generation) | — | — | yes |
| [`conduit_process_resident_bytes`](#conduit_process_resident_bytes), [`conduit_process_open_fds`](#conduit_process_open_fds) | — | — | yes (`full` only, Linux `/proc`) |

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

### conduit_responses_total { #conduit_responses_total }

| | |
|--|--|
| **Type** | Counter |
| **Labels** | `listener`, `protocol`, `rcode_class`, `ip_family` |
| **Profile** | `full` only |
| **When** | [Send](/concepts/architecture-and-packet-path.md#send) completes — upstream answer or synthesized error |

`rcode_class`: `NOERROR`, `NXDOMAIN`, `SERVFAIL`, `OTHER` (and related groupings from the response code).

### Policy drops (no built-in counter) { #policy-drops-no-built-in-counter }

[Request rules](/concepts/architecture-and-packet-path.md#request-rules) or [Response rules](/concepts/architecture-and-packet-path.md#response-rules) **drop** actions, and Rhai **drop** on those hooks, end the [transaction](/glossary/index.md#transaction) with **no DNS reply**. There is no dedicated drop counter; use [event export](/observability/event-export.md) or logs if you need visibility.

---

## Scrape-time gauges

Updated when Prometheus (or OTEL) scrapes — not incremented on listener workers.

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
sum(rate(conduit_responses_total[5m])) by (rcode_class)
sum(rate(conduit_forward_errors_total[5m])) by (pool, reason)
histogram_quantile(0.99, sum(rate(conduit_forward_duration_seconds_bucket[5m])) by (le, pool))
conduit_config_generation
conduit_build_info{revision="abc1234", dirty="false", profile="release"}
```

Prometheus scrape and OTEL push both consume the same metric families from `render_prometheus()`. Histogram `_bucket` series are available on Prometheus scrape; OTLP carries counter, gauge, and histogram summaries for built-ins.
