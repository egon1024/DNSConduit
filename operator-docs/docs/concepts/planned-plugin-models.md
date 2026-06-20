# Planned plugin models

[WASM](/glossary/index.md#wasm), [sidecar](/glossary/index.md#sidecar), and **[processor chains](/processor-chains/index.md)** are **not yet shipped**. This page describes what is planned — not what you can configure today.

**Shipped today:** declarative [rules](/policy-routing/rules-and-actions.md) and **Rhai for rules** ([Rule Rhai](/glossary/index.md#rule-rhai)) on the same request/response hooks. Behavioral home: [Policy & routing](/policy-routing/index.md). Rhai reference: [Rhai for rules](/rhai/rule-rhai.md) under [Rhai](/rhai/index.md). The [Rhai](/rhai/index.md) section covers script APIs; [Processor chains](/processor-chains/index.md) covers planned `processors:` wiring and backends.

## WASM (planned) { #wasm-planned }

**[WASM](/glossary/index.md#wasm)** is a planned in-process plugin model: compiled `.wasm` files loaded inside `conduit` on the same [Request rules](/concepts/architecture-and-packet-path.md#request-rules) and [Response rules](/concepts/architecture-and-packet-path.md#response-rules) hooks as [Rule Rhai](/glossary/index.md#rule-rhai).

Operators would deploy plugin artifacts, grant access to named lookup tables, and rely on Conduit’s sandbox around guest code. Targets near-native speed and stronger isolation for untrusted third-party logic.

Configuration and workflows will be documented when the feature ships.

## Sidecar (planned) { #sidecar-planned }

**[Sidecar](/glossary/index.md#sidecar)** is a planned plugin model: separate processes Conduit calls over gRPC or a Unix-domain socket on the same logical policy hooks as [Rule Rhai](/glossary/index.md#rule-rhai) and [WASM](/glossary/index.md#wasm).

Sidecars trade per-query latency for running code in another language or an existing service. They suit lower-QPS policy paths or teams that prefer a helper service over in-process plugins.

**Not yet shipped.**

## Processor chains (planned) { #processor-chains-planned }

**Processor chains** are a planned datapath feature — separate from `rules:` — documented as a feature section when they ship. **Feature home:** [Processor chains](/processor-chains/index.md).

They attach at named pipeline points (for example after [Request rules](/concepts/architecture-and-packet-path.md#request-rules) and after upstream response) and are the planned home for **DNS wire editing**: ingress query changes (including qname rewrite), egress response mutation, EDNS/EDE, and conveniences such as RD-bit control that Rhai for rules does not provide today.

Backends in a chain may include:

| Backend | Reference |
|---------|-----------|
| [Processor-chain Rhai](/glossary/index.md#processor-chain-rhai) | [Rhai for processor chains](/rhai/processor-chain-rhai.md), [Message API](/rhai/message-api.md) (`conduit-dns`) |
| [WASM](/glossary/index.md#wasm) | This page — WASM (planned) |
| [Sidecar](/glossary/index.md#sidecar) | This page — Sidecar (planned) |

Rhai for rules and Rhai for processor chains are **different** config and APIs; both may coexist when processor chains ship.

### Policy refinement (planned)

Processor chains are **not** a replacement for [rules](/policy-routing/rules-and-actions.md). [Request rules](/concepts/architecture-and-packet-path.md#request-rules) and [Response rules](/concepts/architecture-and-packet-path.md#response-rules) still run first on their hooks. Processors run at fixed pipeline slots **after** those phases and may **refine** the [transaction](/glossary/index.md#transaction) before [Route](/concepts/architecture-and-packet-path.md#route) or before [Response rules](/concepts/architecture-and-packet-path.md#response-rules).

Full table and notes: [Processor chains — policy refinement](/processor-chains/index.md#policy-refinement-planned).

Use **tags** to tie rules and processors to one logical decision: a rule (or Rhai for rules) matches on the **client** query and sets tags; a processor link runs with a matching `when:` guard (or checks tags in script) and rewrites wire or refines pool/tags before routing or response policy.

Rhai for rules cannot set pool from wire inspection after rewrite — processor chains are the planned place for that combined behavior without overloading `rules:`.

## Lookup tables (host feature)

Lookup tables under **`data_sources:`** are host-owned data loaded at [runtime snapshot](/glossary/index.md#runtime-snapshot) build. In current releases, only Rhai for rules calls **`table_lookup`**; planned [WASM](/glossary/index.md#wasm), [sidecar](/glossary/index.md#sidecar), and processor-chain backends would share the same host lookup surface.

Current behavior and config paths: [Data sources and lookups](/rhai/data-sources-and-lookups.md).

## Policy metrics (planned sharing)

In current releases, only Rhai for rules can publish custom policy metrics (`metric_inc` → `conduit_user_*`). See [User metrics](/rhai/user-metrics.md).

| Mechanism | Custom metrics (write) | In current releases |
|-----------|------------------------|---------------------|
| Built-in [rules](/policy-routing/rules-and-actions.md) | No | Yes (rules only) |
| [Rule Rhai](/glossary/index.md#rule-rhai) | Yes | Yes |
| [WASM](/glossary/index.md#wasm) | Planned — same host APIs as Rhai for rules | No |
| [Sidecar](/glossary/index.md#sidecar) | Planned — deltas via hook protocol | No |
| Processor chains | Planned | No |

Built-in [metrics](/observability/metrics.md) on the [dataplane](/glossary/index.md#dataplane) still reflect rule outcomes regardless of plugin model.

## Related

- [Processor chains](/processor-chains/index.md)
- [Policy & routing](/policy-routing/index.md) — declarative policy and Rhai for rules today
- [Rhai](/rhai/index.md) — Rhai for rules and Rhai for processor chains
- [Architecture and packet path](/concepts/architecture-and-packet-path.md) — pipeline phases
