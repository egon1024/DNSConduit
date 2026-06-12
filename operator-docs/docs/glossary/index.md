# Glossary

Brief definitions of terminology used in this documentation. Each entry links to the canonical topic page for detail.

When writing other pages, link here often — for example `[overlay](/glossary/index.md#overlay)` or `[Rhai](/rhai/index.md)`.

Entries are added as documentation grows. Keep one-line glosses in sync when linked pages change.

## Config model

### Overlay

<!-- one-line definition -->

→ [Configuration model](../control-plane/configuration-model.md)

### File layer

<!-- one-line definition -->

→ [Configuration model](../control-plane/configuration-model.md)

### Effective config

<!-- one-line definition -->

→ [Configuration model](../control-plane/configuration-model.md)

### Export

<!-- one-line definition -->

→ [Reload and export](../control-plane/reload-and-export.md)

### File-wins reload

<!-- one-line definition -->

→ [Reload and export](../control-plane/reload-and-export.md)

## Runtime

### Runtime snapshot

The validated settings bundle the [dataplane](/glossary/index.md#dataplane) uses to answer queries at a given moment — effective config (listeners, pools, forward behavior), loaded rules and scripts, and observability filters. All listener workers share the same snapshot until you reload or apply new settings.

→ [Architecture and packet path](../concepts/architecture-and-packet-path.md)

### Last-good snapshot

The previous working [runtime snapshot](/glossary/index.md#runtime-snapshot) Conduit keeps when a reload or apply fails validation — DNS continues on the last known-good settings instead of the rejected change.

→ [Reload and export](../control-plane/reload-and-export.md)

### Pending reconcile

<!-- one-line definition -->

→ [Reload and export](../control-plane/reload-and-export.md)

## Datapath

### Dataplane

The `conduit` service and query-processing runtime: configured listeners accept client DNS traffic, each query runs through the pipeline as a [transaction](/glossary/index.md#transaction), and responses come from upstream [backends](/glossary/index.md#backend). Distinct from the optional [control plane](/glossary/index.md#control-plane), which exposes gRPC and `conduitctl` when enabled.

→ [Architecture and packet path](../concepts/architecture-and-packet-path.md)

### Transaction

Everything Conduit remembers for one client query on the [dataplane](/glossary/index.md#dataplane) — the question, client and listener context, [tags](/glossary/index.md#tags), selected [pool](/glossary/index.md#pool) and [backend](/glossary/index.md#backend), query and response messages, and optional trace — from [Receive](/concepts/architecture-and-packet-path.md#receive) through [Send](/concepts/architecture-and-packet-path.md#send) or drop.

→ [Architecture and packet path](../concepts/architecture-and-packet-path.md)

### Tags

Runtime key/value annotations on a [transaction](/glossary/index.md#transaction), set or tested by rules and scripts; persist across [retries](/glossary/index.md#retry) unless cleared. Not part of on-disk config export.

→ [Architecture and packet path](../concepts/architecture-and-packet-path.md)

### Retry

Another [Route](/concepts/architecture-and-packet-path.md#route) → [Forward](/concepts/architecture-and-packet-path.md#forward) cycle for the same client [transaction](/glossary/index.md#transaction), triggered from [Response rules](/concepts/architecture-and-packet-path.md#response-rules) via the `retry` or `retry_pool` [action](/glossary/index.md#action) (or [Rhai](/glossary/index.md#rhai)); capped by `orchestrator.max_attempts`, `orchestrator.max_txn_duration_ms`, and pool exhaustion when every [backend](/glossary/index.md#backend) in the target [pool](/glossary/index.md#pool) was already tried.

→ [Retries and transactions](../policy-routing/retries-and-transactions.md)

### Pool

Named group of [backends](/glossary/index.md#backend); [rules](/glossary/index.md#selector) and scripts select a pool by name before Conduit picks a backend to forward to.

→ [Pools and backends](/policy-routing/pools-and-backends.md)

### Backend

Configured upstream destination Conduit forwards DNS queries to; settings control how Conduit reaches and uses that destination (for example address and weight in current releases).

→ [Pools and backends](/policy-routing/pools-and-backends.md)

### Selector

Condition on a [rule](/policy-routing/rules-and-actions.md) that tests query or response fields (for example query name, type, response code, or [tag](/glossary/index.md#tags) presence). Conduit evaluates rules in first-match order on each hook.

→ [Rules and actions](/policy-routing/rules-and-actions.md)

### Action

Built-in effect on a matching [rule](/policy-routing/rules-and-actions.md) (for example `set_pool`, `set_tag`, `set_source_v4`, `set_source_v6`, `drop`, `retry`, `retry_pool`, `rhai`) — applied before optional [Rhai](/glossary/index.md#rhai) on the same rule.

→ [Rules and actions](/policy-routing/rules-and-actions.md)

## Extensibility

### Rhai

Scripting plugin model in current releases: `.rhai` files referenced from `rhai` [actions](/glossary/index.md#action) on [rules](/policy-routing/rules-and-actions.md), loaded into the [runtime snapshot](/glossary/index.md#runtime-snapshot) on reload or apply, run at [Request rules](/concepts/architecture-and-packet-path.md#request-rules) and [Response rules](/concepts/architecture-and-packet-path.md#response-rules) within [sandbox limits](/rhai/sandbox-limits.md).

→ [Rhai](/rhai/index.md), [Extensibility](/concepts/extensibility.md)

### WASM

Planned in-process plugin model: compiled `.wasm` plugins on the same request/response hooks as [Rhai](/glossary/index.md#rhai). **Not available** in current releases.

→ [Extensibility](/concepts/extensibility.md)

### Sidecar

Planned sidecar plugin model: separate processes Conduit calls on the same logical hooks as [Rhai](/glossary/index.md#rhai) and [WASM](/glossary/index.md#wasm). **Not available** in current releases.

→ [Extensibility](/concepts/extensibility.md)

## Control and operations

### Control plane

Optional gRPC API and operator tools (`conduitctl`, reload, export). Separate from the DNS [dataplane](/glossary/index.md#dataplane), which serves queries whether or not control is enabled.

→ [Control plane](../control-plane/index.md)

### conduitctl

<!-- one-line definition -->

→ [gRPC and conduitctl](../control-plane/grpc-and-conduitctl.md)

## Observability

### Event sink

<!-- one-line definition -->

→ [Event export](../observability/event-export.md)

### dnstap

<!-- one-line definition -->

→ [Event export](../observability/event-export.md)

### Pipeline trace

<!-- one-line definition -->

→ [Tracing](../observability/tracing.md)
