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

<!-- one-line definition -->

→ [Configuration model](../control-plane/configuration-model.md)

### Last-good snapshot

<!-- one-line definition -->

→ [Reload and export](../control-plane/reload-and-export.md)

### Pending reconcile

<!-- one-line definition -->

→ [Reload and export](../control-plane/reload-and-export.md)

## Datapath

### Dataplane

The `conduit` service and query-processing runtime: configured listeners accept client DNS traffic, each query runs through the pipeline as a [transaction](/glossary/index.md#transaction), and responses come from upstream [backends](/glossary/index.md#backend). Distinct from the optional [control plane](/glossary/index.md#control-plane), which exposes gRPC and `conduitctl` when enabled.

→ [Architecture and packet path](../concepts/architecture-and-packet-path.md)

### Transaction

<!-- one-line definition -->

→ [Architecture and packet path](../concepts/architecture-and-packet-path.md)

### Tags

<!-- one-line definition -->

→ [Architecture and packet path](../concepts/architecture-and-packet-path.md)

### Retry

<!-- one-line definition -->

→ [Retries and transactions](../policy-routing/retries-and-transactions.md)

### Pool

Named group of [backends](/glossary/index.md#backend); [rules](/glossary/index.md#selector) and scripts select a pool by name before Conduit picks a backend to forward to.

→ [Pools and backends](/policy-routing/pools-and-backends.md)

### Backend

Configured upstream destination Conduit forwards DNS queries to; settings control how Conduit reaches and uses that destination (for example address and weight in current releases).

→ [Pools and backends](/policy-routing/pools-and-backends.md)

### Selector

<!-- one-line definition -->

→ [Rules and actions](../policy-routing/rules-and-actions.md)

### Action

<!-- one-line definition -->

→ [Rules and actions](../policy-routing/rules-and-actions.md)

## Extensibility

### Rhai

<!-- one-line definition -->

→ [Rhai](../rhai/index.md)

### WASM

<!-- one-line definition -->

→ [Extensibility](../concepts/extensibility.md)

### Sidecar

<!-- one-line definition -->

→ [Extensibility](../concepts/extensibility.md)

## Control and operations

### Control plane

Optional gRPC API and operator surface (`conduitctl`, reload, export). Separate from the DNS [dataplane](/glossary/index.md#dataplane), which serves queries whether or not control is enabled.

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
