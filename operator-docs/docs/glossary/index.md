---
toc_depth: 3
toc_collapsible: true
---

# Glossary

Brief definitions of terminology used in this documentation. Each entry links to the canonical topic page for detail.

When writing other pages, link here often — for example `[overlay](/glossary/index.md#overlay)` or `[Rhai](/rhai/index.md)`.

Entries are added as documentation grows. Keep one-line glosses in sync when linked pages change.

## Config model

### Overlay

In-memory config patch applied with `conduitctl apply`; **merge** mode accumulates successive patches, then combines with the [file layer](/glossary/index.md#file-layer) into [effective config](/glossary/index.md#effective-config). Cleared by [reload from disk](/glossary/index.md#reload-from-disk), **`conduitctl apply --clear`**, or **`conduitctl apply --replace`** with an empty patch (`schema_version` only).

→ [Configuration model](/control-plane/configuration-model.md)

### Clear overlay (without reload)

**`conduitctl apply --clear`**: drop the active [overlay](/glossary/index.md#overlay) without re-reading the [file layer](/glossary/index.md#file-layer) from disk — distinct from [reload from disk](/glossary/index.md#reload-from-disk), which re-reads the startup file and clears the overlay.

→ [Reload and export](/control-plane/reload-and-export.md#clear-vs-reload)

### File layer

YAML read from the path passed at `conduit` startup — the durable baseline on disk, distinct from any API [overlay](/glossary/index.md#overlay).

→ [Configuration model](/control-plane/configuration-model.md)

### Effective config

[File layer](/glossary/index.md#file-layer) after merge with the active [overlay](/glossary/index.md#overlay) (if any) and before compile into a [runtime snapshot](/glossary/index.md#runtime-snapshot).

→ [Configuration model](/control-plane/configuration-model.md)

### Export

YAML serialization of the **[effective config](/glossary/index.md#effective-config)** (file layer plus active overlay, defaults normalized) via `conduitctl export` or gRPC `ExportConfig`.

→ [Reload and export](/control-plane/reload-and-export.md)

### Reload from disk

**SIGHUP** or **`conduitctl reload`**: re-read the startup config file from disk, **clear the overlay**, validate, and swap the [runtime snapshot](/glossary/index.md#runtime-snapshot). Afterward, effective config comes from the on-disk file only.

→ [Reload and export](/control-plane/reload-and-export.md)

## Runtime

### Runtime snapshot

The validated settings bundle the [dataplane](/glossary/index.md#dataplane) uses to answer queries at a given moment — effective config (listeners, pools, forward behavior), loaded rules and scripts, and observability filters. All listener workers share the same snapshot until you reload or apply new settings.

→ [Architecture and packet path](../concepts/architecture-and-packet-path.md)

### Last-good snapshot

The previous working [runtime snapshot](/glossary/index.md#runtime-snapshot) Conduit keeps when a reload or apply fails validation — DNS continues on the last known-good settings instead of the rejected change.

→ [Reload and export](/control-plane/reload-and-export.md)

### Pending reconcile

Snapshot updated after a reload or apply, but `listeners` or `forward` socket state still reflects the previous process start — restart `conduit` to apply on the wire.

→ [Configuration model](/control-plane/configuration-model.md)

### Runtime model

The [dataplane](/glossary/index.md#dataplane) execution model chosen **once at process startup** with **`dataplane.runtime`** — **`sync`** (default) or **`split_io`** — deciding *how* the per-query [pipeline](/concepts/architecture-and-packet-path.md#pipeline-phases) is spread across OS threads. It does not change the pipeline phases; changing it (or its worker counts) requires a **restart**.

→ [Runtime and concurrency](/concepts/runtime-and-concurrency.md#runtime-models)

### Ingress worker

OS thread that accepts a client DNS message on a [listener](/glossary/index.md#listener) (count from **`listeners.threads`**). Under **`sync`** it runs the whole pipeline including the upstream wait; under **`split_io`** it does the structural parse, takes a [transaction slot](/glossary/index.md#transaction-slot-pool), and hands off without blocking on upstream.

→ [Runtime and concurrency](/concepts/runtime-and-concurrency.md#split-io-runtime)

### Policy worker

Under **`split_io`**, a thread (count from **`dataplane.policy_workers`**) that runs the orchestrator phases — [Request rules](/concepts/architecture-and-packet-path.md#request-rules), [Route](/concepts/architecture-and-packet-path.md#route), the [Forward](/concepts/architecture-and-packet-path.md#forward) submit — and finishes each [transaction](/glossary/index.md#transaction) at [Response rules](/concepts/architecture-and-packet-path.md#response-rules) / [Send](/concepts/architecture-and-packet-path.md#send) once a reply is in.

→ [Runtime and concurrency](/concepts/runtime-and-concurrency.md#split-io-runtime)

### I/O worker

Under **`split_io`**, a thread (count from **`dataplane.io_workers`**) that owns the upstream sockets: it matches incoming replies to **parked** transactions, enforces `forward.timeout_ms`, and resumes each transaction at [Wait for response](/concepts/architecture-and-packet-path.md#wait-for-response). One I/O worker handles many concurrent parked waits.

→ [Runtime and concurrency](/concepts/runtime-and-concurrency.md#split-io-runtime)

### Transaction slot pool

Preallocated arena of [transaction](/glossary/index.md#transaction) slots, shared by both [runtime models](/glossary/index.md#runtime-model), that grows in chunks (**`dataplane.slot_chunk_size`**) up to **`orchestrator.txn_table_capacity`**. A query holds one slot from [Receive](/concepts/architecture-and-packet-path.md#receive) to [Send](/concepts/architecture-and-packet-path.md#send) — including while a `split_io` query is parked waiting upstream. When all slots are in use Conduit applies backpressure and increments [`conduit_slot_pool_exhausted_total`](/observability/built-in-metrics.md#conduit_slot_pool_exhausted_total).

→ [Runtime and concurrency](/concepts/runtime-and-concurrency.md#transaction-slot-pool)

## Datapath

### Dataplane

The `conduit` service and query-processing runtime: configured listeners accept client DNS traffic, each query runs through the pipeline as a [transaction](/glossary/index.md#transaction), and responses come from upstream [backends](/glossary/index.md#backend). Distinct from the optional [control plane](/glossary/index.md#control-plane), which exposes gRPC and `conduitctl` when enabled.

→ [Architecture and packet path](../concepts/architecture-and-packet-path.md)

### Listener

A configured client-facing DNS bind address (`listeners.listeners[]`: `address` + `protocol` `udp` or `tcp`). Distinct from the optional gRPC **`control:`** listener.

→ [Reference: listeners](/reference/config-schema/listeners.md)

### Transaction

Everything Conduit remembers for one client query on the [dataplane](/glossary/index.md#dataplane) — the question, client and listener context, [tags](/glossary/index.md#tags), selected [pool](/glossary/index.md#pool) and [backend](/glossary/index.md#backend), query and response messages, and optional trace — from [Receive](/concepts/architecture-and-packet-path.md#receive) through [Send](/concepts/architecture-and-packet-path.md#send) or drop.

→ [Architecture and packet path](../concepts/architecture-and-packet-path.md)

### Tags

Runtime key/value annotations on a [transaction](/glossary/index.md#transaction), set or tested by rules and scripts; persist across [retries](/glossary/index.md#retry) unless cleared. Not part of on-disk config export.

→ [Architecture and packet path](../concepts/architecture-and-packet-path.md)

### Retry

Another [Route](/concepts/architecture-and-packet-path.md#route) → [Forward](/concepts/architecture-and-packet-path.md#forward) cycle for the same client [transaction](/glossary/index.md#transaction), triggered from [Response rules](/concepts/architecture-and-packet-path.md#response-rules) via the `retry` or `retry_now` [action](/glossary/index.md#action) (or [Rhai](/glossary/index.md#rhai)); capped by `orchestrator.max_attempts`, `orchestrator.max_txn_duration_ms`, and pool exhaustion when every [backend](/glossary/index.md#backend) in the target [pool](/glossary/index.md#pool) was already tried.

→ [Retries and transactions](../policy-routing/retries-and-transactions.md)

### Pool

Named group of [backends](/glossary/index.md#backend); [rules](/glossary/index.md#selector) and scripts select a pool by name before Conduit picks a backend to forward to.

→ [Pools and backends](/policy-routing/pools-and-backends.md)

### Backend

Configured upstream destination Conduit forwards DNS queries to; settings control how Conduit reaches and uses that destination (for example address and weight in current releases).

→ [Pools and backends](/policy-routing/pools-and-backends.md)

### EWMA

**Exponentially weighted moving average** — a smoothed statistic that blends each new measurement with prior history, giving **more weight to recent samples** than to older ones. Conduit updates a per-[backend](/glossary/index.md#backend) **latency EWMA** from successful health-probe round-trip times. That value reflects how fast the backend has been responding lately (not a single spike or one slow query). [Route](/concepts/architecture-and-packet-path.md#route) uses the EWMA — through a damped **`weight_factor`** — to reduce traffic share to slower but still-eligible backends without removing them from the pool.

→ [Runtime API — `latency_ewma_ms`](/rhai/runtime-api.md#backendruntimelatency_ewma_ms), [`min_latency_ewma_ms`](/rhai/runtime-api.md#poolruntimemin_latency_ewma_ms)

### Selector

Condition on a [rule](/policy-routing/rules-and-actions.md) that tests query or response fields (for example query name, type, response code, or [tag](/glossary/index.md#tags) presence). Conduit evaluates rules in first-match order on each hook.

→ [Rules and actions](/policy-routing/rules-and-actions.md)

### RCODE

DNS response code for the current forward attempt (for example **SERVFAIL**, **NOERROR**). [Response rules](/concepts/architecture-and-packet-path.md#response-rules) match on `rcode` selectors; [Rule Rhai](/glossary/index.md#rule-rhai) uses `txn.response_rcode()` on the response hook.

→ [Rules and actions — Selectors](/policy-routing/rules-and-actions.md#selectors), [Transaction API — Query and response](/rhai/txn-api.md#query-and-response)

### sample_percent

Deterministic sampling on a **`0..100`** scale. `0` never matches; `100` always matches. By default the hash uses the transaction id only. Optional **`key`** (static string) or **`key_from`** (`qname`, `rule_name` on rules, `sink_name` on event sinks) selects an independent bucket namespace — see [Sampling and cadence](/policy-routing/rules-and-actions.md#sampling-and-cadence).

On [rules](/policy-routing/rules-and-actions.md), use selector type **`sample_percent`** with optional `key` / `key_from`, or **`every_nth_worker`** / **`every_nth_global`**. On [tracing](/observability/tracing.md) and [event export](/observability/event-export.md), use top-level **`sample_percent`** with optional **`sample_key`** / **`sample_key_from`**. Rhai: [`txn.sample_percent`](/rhai/txn-api.md#txnsample_percent) and related methods on [Transaction API — Sampling](/rhai/txn-api.md#sampling).

→ [Sampling and cadence](/policy-routing/rules-and-actions.md#sampling-and-cadence), [Event export](/observability/event-export.md), [Tracing](/observability/tracing.md)

### every_nth selectors

Rule selectors that match every Nth query: **`every_nth_worker`** uses the worker-local transaction id; **`every_nth_global`** uses a process-wide query index incremented once per query. Rules only — not valid on tracing or event filter selectors.

→ [Sampling and cadence](/policy-routing/rules-and-actions.md#sampling-and-cadence)

### Action

Built-in effect on a matching [rule](/policy-routing/rules-and-actions.md) (for example `set_pool`, `set_retry_pool`, `set_tag`, `set_source_v4`, `set_source_v6`, `set_retry_source_v4`, `set_retry_source_v6`, `clear_retry_source_v4`, `clear_retry_source_v6`, `drop`, `drop_now`, `clear_drop`, `retry`, `retry_now`, `clear_retry`, `clear_retry_pool`, `rhai`) — run in **list order** on the matching rule.

→ [Rules and actions](/policy-routing/rules-and-actions.md)

### Rule Rhai

**Also called:** Rhai for rules.

Scripted **policy** on [rules](/policy-routing/rules-and-actions.md) in current releases: `.rhai` files referenced from `rhai` [actions](/glossary/index.md#action), loaded into the [runtime snapshot](/glossary/index.md#runtime-snapshot) on reload or apply, run at [Request rules](/concepts/architecture-and-packet-path.md#request-rules) and [Response rules](/concepts/architecture-and-packet-path.md#response-rules) within [sandbox limits](/rhai/sandbox-limits.md). Uses the `txn` API — not DNS wire editing.

→ [Rule Rhai](/rhai/rule-rhai.md), [Rhai](/rhai/index.md), [Rules and actions](/policy-routing/rules-and-actions.md)

### Rhai

Embedded scripting in Conduit — [Rule Rhai](/glossary/index.md#rule-rhai) on `rules:` for policy on the request and response hooks.

→ [Rhai](/rhai/index.md)

## Control and operations

### Control plane

Optional gRPC API and operator tools (`conduitctl`, reload, export). Separate from the DNS [dataplane](/glossary/index.md#dataplane), which serves queries whether or not control is enabled.

→ [Control plane](../control-plane/index.md)

### conduitctl

CLI for the [control plane](/glossary/index.md#control-plane) — `apply` (with merge / replace / clear modes), `export`, `reload`, `validate`, and `trace`.

→ [gRPC and conduitctl](/control-plane/grpc-and-conduitctl.md)

## Observability

### Event sink

Configured destination (for example a [dnstap](#dnstap) collector) that receives per-query observation frames from the [dataplane](/glossary/index.md#dataplane).

→ [Event export](/observability/event-export.md)

### dnstap

Industry-standard protobuf format and framestream transport for DNS observation; Conduit exports client query/response (and optional retry) frames when `events.sinks` includes `type: dnstap`.

→ [Event export](/observability/event-export.md)

### Pipeline trace

In-memory record of pipeline phases, timing, and routing decisions for one [transaction](/glossary/index.md#transaction); fetched via `GetTrace` / `conduitctl trace` when [tracing](/observability/tracing.md) is enabled.

→ [Tracing](/observability/tracing.md)
