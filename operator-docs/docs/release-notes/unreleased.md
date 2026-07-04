# Unreleased

Changes merged to `main` that are not yet tagged. Staging area for the next operator release.

## Breaking changes — Rule Rhai host API { #breaking-changes--rule-rhai-host-api }

Rule Rhai scripts **must be updated** before upgrading to this release. Legacy host symbols are **removed**; there are no compatibility aliases.

Run **`conduitctl validate --file <config>`** after updating every `.rhai` script referenced in your config.

### Migration table

| Legacy | New | Notes |
|--------|-----|-------|
| `table_lookup(table, key)` | `lookup(table, key)` | Global function; same lookup semantics |
| `txn.metric_inc(name, delta)` | `metrics.inc(name, delta)` | `metrics` object is in hook scope |
| `txn.metric_inc_labels(name, delta, labels)` | `metrics.inc_labels(name, delta, labels)` | Same |
| `log_info(msg)` | `log.info(msg)` | `log` object in hook scope |
| `log_warn(msg)` | `log.warn(msg)` | Same |
| `question_qname(txn)` | `txn.question().qname` | Use `txn.question()` when you also need qtype, class, or opcode |

### New read-only surface: `runtime.routing`

On pools with health enabled, scripts can read Route-consistent routing/health state **at hook entry**:

- **`runtime.routing().pool(name)`** — `configured_count`, `eligible_count`, `fail_open_active`, and pool aggregates (minimum [latency EWMA](/glossary/index.md#ewma) via `min_latency_ewma_ms`, `max_outstanding`)
- **`runtime.routing().backend(pool, id)`** — `applied`, `observed`, `eligible`, `frozen`, [latency EWMA](/glossary/index.md#ewma) (`latency_ewma_ms`), `weight_factor`, `outstanding`, `last_transition_unix_ms` (`id` = backend `name` or `ip:port`)
- **`runtime.routing().backend_for_attempt(pool, backend_id)`** — same backend view for a named attempt; pass `txn.selected_pool()` and `txn.selected_backend_name()` for the current attempt

Use **method call** syntax: `runtime.routing().pool("default")`, not `runtime.routing.pool(...)`.

Values reflect host state at **hook entry** on this worker (the same sources [Route](/concepts/architecture-and-packet-path.md#route) uses). There is no stronger transactional guarantee against concurrent probe or passive health updates.

Unknown pool or backend at runtime returns an **empty view** (documented sentinel field values).

### Unchanged on `txn`

Policy effects remain on **`txn`**: `set_pool`, tags, retry/drop, egress overrides, `question()`, `response()`, sampling helpers, `selected_*`, and related methods.

### Operator action

1. Update every rule `.rhai` file using the migration table above.
2. Re-run **`conduitctl validate`** on each config before reload.
3. See [Host API overview](/rhai/host-api.md), [Runtime API](/rhai/runtime-api.md), [Data sources and lookups](/rhai/data-sources-and-lookups.md), and [User metrics](/rhai/user-metrics.md) for the updated reference.

---

## Backend health

Shipped in the **same release** as the Rhai host API changes above:

- Active health probing, passive fast-trip, and health-aware [Route](/concepts/architecture-and-packet-path.md#route) selection (including latency [EWMA](/glossary/index.md#ewma)-weighted backend shares) — [Backend health](/policy-routing/backend-health.md)
- Operator controls via **`conduitctl health`** (show, set down/up, freeze, resume) — [gRPC and conduitctl — health](/control-plane/grpc-and-conduitctl.md#health)
- Health-related Prometheus metrics when `metrics.profile: full` — [Built-in metrics — Backend health](/observability/built-in-metrics.md#backend-health)
- Config: [`pools[].health`](/reference/config-schema/health.md)

Scripts read live health through **`runtime.routing`** on the hot path — not control-plane RPC per query. See [Runtime API — routing](/rhai/runtime-api.md#routingruntime).

### Health and forward metrics

- **[`conduit_probe_results_total`](/observability/built-in-metrics.md#conduit_probe_results_total)** (`full` only) — active health-probe outcomes per backend (`success`, `failure`, `timeout`, `send_error`).
- **[`conduit_forward_errors_total`](/observability/built-in-metrics.md#conduit_forward_errors_total)** now includes a **`backend`** label (`pool`, `backend`, `reason`) so timeouts and other forward errors are attributable per upstream. Existing PromQL that only groups by `pool` / `reason` still works; series identity gains the new label.
- Passive fast-trip logs each counting failure and the trip event at WARN with query and client context — [Backend health](/policy-routing/backend-health.md).
