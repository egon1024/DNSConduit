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

### Reload from disk { #reload-from-disk #file-wins-reload }

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

### sample_percent

Deterministic selector/filter sampling control with a **`0..100`** percentage scale. `0` never matches and `100` always matches for sampling checks. On [rules](/policy-routing/rules-and-actions.md), use selector type **`sample_percent`**. On [tracing](/observability/tracing.md) and [event export](/observability/event-export.md), use top-level **`sample_percent`** on activation or sink filters.

→ [Sampling and cadence](/policy-routing/rules-and-actions.md#sampling-and-cadence), [Event export](/observability/event-export.md), [Tracing](/observability/tracing.md)

### every_nth selectors

Rule selectors that match every Nth query: **`every_nth_worker`** uses the worker-local transaction id; **`every_nth_global`** uses a process-wide query index incremented once per query. Rules only — not valid on tracing or event filter selectors.

→ [Sampling and cadence](/policy-routing/rules-and-actions.md#sampling-and-cadence)

### Action

Built-in effect on a matching [rule](/policy-routing/rules-and-actions.md) (for example `set_pool`, `set_tag`, `set_source_v4`, `set_source_v6`, `drop`, `retry`, `retry_pool`, `rhai`) — applied before optional [Rule Rhai](/glossary/index.md#rule-rhai) on the same rule.

→ [Rules and actions](/policy-routing/rules-and-actions.md)

### Rule Rhai { #rule-rhai #rhai }

Scripted **policy** on [rules](/policy-routing/rules-and-actions.md) in current releases: `.rhai` files referenced from `rhai` [actions](/glossary/index.md#action), loaded into the [runtime snapshot](/glossary/index.md#runtime-snapshot) on reload or apply, run at [Request rules](/concepts/architecture-and-packet-path.md#request-rules) and [Response rules](/concepts/architecture-and-packet-path.md#response-rules) within [sandbox limits](/rhai/sandbox-limits.md). Uses the `txn` API — not DNS wire editing.

→ [Rhai](/rhai/index.md), [Rules and actions](/policy-routing/rules-and-actions.md)

## Planned plugin models

### WASM

Planned in-process plugin model: compiled `.wasm` plugins on the same request/response hooks as [Rule Rhai](/glossary/index.md#rule-rhai). **Not yet shipped.**

→ [Planned plugin models](/concepts/planned-plugin-models.md)

### Sidecar

Planned sidecar plugin model: separate processes Conduit calls on the same logical hooks as [Rule Rhai](/glossary/index.md#rule-rhai) and [WASM](/glossary/index.md#wasm). **Not yet shipped.**

→ [Planned plugin models](/concepts/planned-plugin-models.md)

### Processor chains

Planned datapath feature with `processors:` config: DNS wire editing and (when shipped) transaction refinement such as **`set_tag`** and ingress **`set_pool`** after [rules](/policy-routing/rules-and-actions.md) — separate from [Rule Rhai](/glossary/index.md#rule-rhai). **Not yet shipped.**

→ [Planned plugin models](/concepts/planned-plugin-models.md#processor-chains-planned)

### Processor-chain Rhai

Planned [Rhai](/glossary/index.md#rhai) scripts in [processor chains](/glossary/index.md#processor-chains): `processors:` config and **message API** (`conduit-dns`) for wire editing — not the rule `txn` surface. **Not yet shipped.**

→ [Planned plugin models](/concepts/planned-plugin-models.md#processor-chains-planned)

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
