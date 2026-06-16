# Planned plugin models

[WASM](/glossary/index.md#wasm), [sidecar](/glossary/index.md#sidecar), and **processor chains** are **not yet shipped**. This page describes what is planned — not what you can configure today.

**Shipped today:** declarative [rules](/policy-routing/rules-and-actions.md) and **[Rule Rhai](/glossary/index.md#rule-rhai)** on the same request/response hooks. That is documented under [Policy & routing](/policy-routing/index.md) and [Rhai](/rhai/index.md), not here.

## WASM (planned)

**[WASM](/glossary/index.md#wasm)** is a planned in-process plugin model: compiled `.wasm` files loaded inside `conduit` on the same [Request rules](/concepts/architecture-and-packet-path.md#request-rules) and [Response rules](/concepts/architecture-and-packet-path.md#response-rules) hooks as [Rule Rhai](/glossary/index.md#rule-rhai).

Operators would deploy plugin artifacts, grant access to named lookup tables, and rely on Conduit’s sandbox around guest code. Targets near-native speed and stronger isolation for untrusted third-party logic.

Configuration and workflows will be documented when the feature ships.

## Sidecar (planned)

**[Sidecar](/glossary/index.md#sidecar)** is a planned plugin model: separate processes Conduit calls over gRPC or a Unix-domain socket on the same logical policy hooks as [Rule Rhai](/glossary/index.md#rule-rhai) and [WASM](/glossary/index.md#wasm).

Sidecars trade per-query latency for running code in another language or an existing service. They suit lower-QPS policy paths or teams that prefer a helper service over in-process plugins.

**Not yet shipped.**

## Processor chains (planned)

**Processor chains** are a planned datapath feature — separate from `rules:` — with their own `processors:` config and named pipeline attachment points (for example after [Request rules](/concepts/architecture-and-packet-path.md#request-rules) and after upstream response).

They are the planned home for **DNS wire editing**: ingress query changes (including qname rewrite), egress response mutation, EDNS/EDE, and conveniences such as RD-bit control that [Rule Rhai](/glossary/index.md#rule-rhai) does not provide today. Processor chains may use [Processor-chain Rhai](/glossary/index.md#processor-chain-rhai) with a **message API** (`conduit-dns`), not the rule `txn` policy surface. The same chain links are planned to support **WASM** and **sidecar** backends when those runtimes ship.

[Rule Rhai](/glossary/index.md#rule-rhai) and [Processor-chain Rhai](/glossary/index.md#processor-chain-rhai) are **different** config and APIs; both may coexist when processor chains ship.

### Policy refinement (planned)

Processor chains are **not** a replacement for [rules](/policy-routing/rules-and-actions.md). [Request rules](/concepts/architecture-and-packet-path.md#request-rules) and [Response rules](/concepts/architecture-and-packet-path.md#response-rules) still run first on their hooks. Processors run at fixed pipeline slots **after** those phases and may **refine** the [transaction](/glossary/index.md#transaction) before [Route](/concepts/architecture-and-packet-path.md#route) or before [Response rules](/concepts/architecture-and-packet-path.md#response-rules).

When processor chains ship, links are planned to support **policy effects** on the shared transaction in addition to wire edits:

| Effect | Ingress (`post_request_rules`) | Egress (`post_wait`) | Notes |
|--------|-------------------------------|----------------------|--------|
| **`set_tag`** | Planned | Planned | [Tags](/glossary/index.md#tags) set here are visible to later hooks — for example [response rules](/policy-routing/rules-and-actions.md) `tag` selectors after `post_wait` |
| **`set_pool`** | Planned | — | Applies before [Route](/concepts/architecture-and-packet-path.md#route); may override pool chosen in request rules |
| **Drop** | Planned | Planned | Same semantics as rule `drop` |
| **Retry** | — | Planned | Re-enters [Route](/concepts/architecture-and-packet-path.md#route) (egress only), like response-hook `retry` |

Use **tags** to tie rules and processors to one logical decision: a rule (or [Rule Rhai](/glossary/index.md#rule-rhai)) matches on the **client** query and sets tags; a processor link runs with a matching `when:` guard (or checks tags in script) and rewrites wire or refines pool/tags before routing or response policy.

[Rule Rhai](/glossary/index.md#rule-rhai) cannot set pool from wire inspection after rewrite — processor chains are the planned place for that combined behavior without overloading `rules:`.

## Lookup tables (host feature)

Lookup tables under **`data_sources:`** are host-owned data loaded at [runtime snapshot](/glossary/index.md#runtime-snapshot) build. In current releases, only [Rule Rhai](/glossary/index.md#rule-rhai) calls **`table_lookup`**; planned [WASM](/glossary/index.md#wasm) and [sidecar](/glossary/index.md#sidecar) plugins would share the same host lookup surface.

Current behavior and config paths: [Data sources and lookups](/rhai/data-sources-and-lookups.md).

## Policy metrics (planned sharing)

In current releases, only [Rule Rhai](/glossary/index.md#rule-rhai) can publish custom policy metrics (`metric_inc` → `conduit_user_*`). See [User metrics](/rhai/user-metrics.md).

| Mechanism | Custom metrics (write) | In current releases |
|-----------|------------------------|---------------------|
| Built-in [rules](/policy-routing/rules-and-actions.md) | No | Yes (rules only) |
| [Rule Rhai](/glossary/index.md#rule-rhai) | Yes | Yes |
| [WASM](/glossary/index.md#wasm) | Planned — same host APIs as Rule Rhai | No |
| [Sidecar](/glossary/index.md#sidecar) | Planned — deltas via hook protocol | No |
| Processor chains | Planned | No |

Built-in [metrics](/observability/metrics.md) on the [dataplane](/glossary/index.md#dataplane) still reflect rule outcomes regardless of plugin model.

## Related

- [Policy & routing](/policy-routing/index.md) — declarative and scripted policy today
- [Rhai](/rhai/index.md) — Rule Rhai reference
- [Architecture and packet path](/concepts/architecture-and-packet-path.md) — pipeline phases
