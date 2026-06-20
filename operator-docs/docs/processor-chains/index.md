# Processor chains

**Not yet shipped.** Processor chains are a planned datapath feature — separate from `rules:` — with their own **`processors:`** config and named pipeline attachment points.

This section is the **feature home** for processor chains: where they run on the query path, how operators wire chains, and which backends are available ([Rhai for processor chains](/rhai/processor-chain-rhai.md), [WASM](/glossary/index.md#wasm), [sidecar](/glossary/index.md#sidecar)). **Rhai script APIs** live under [Rhai](/rhai/index.md), not here.

## What processor chains are for

Processor chains are the planned home for **DNS wire editing** and related datapath work that Rhai for rules does not cover today:

- Ingress query changes (including qname rewrite)
- Egress response mutation
- EDNS / EDE and conveniences such as RD-bit control

They are **not** a replacement for [rules](/policy-routing/rules-and-actions.md). [Request rules](/concepts/architecture-and-packet-path.md#request-rules) and [Response rules](/concepts/architecture-and-packet-path.md#response-rules) still run first on their hooks. Processor links run at fixed pipeline slots **after** those phases and may refine the shared [transaction](/glossary/index.md#transaction) before [Route](/concepts/architecture-and-packet-path.md#route) or before response policy.

[Rhai for rules](/rhai/rule-rhai.md) and processor chains may coexist: for example a rule tags a query on the client qname; a processor rewrites wire and sets pool from the rewritten name before Route.

## Backends (planned)

| Backend | Doc home |
|---------|----------|
| Rhai (message API) | [Rhai for processor chains](/rhai/processor-chain-rhai.md), [Message API](/rhai/message-api.md) |
| WASM | [Planned plugin models — WASM](/concepts/planned-plugin-models.md#wasm-planned) |
| Sidecar | [Planned plugin models — Sidecar](/concepts/planned-plugin-models.md#sidecar-planned) |

## Policy refinement (planned) { #policy-refinement-planned }

Processor links are planned to support **policy effects** on the shared transaction in addition to wire edits:

| Effect | Ingress (`post_request_rules`) | Egress (`post_wait`) | Notes |
|--------|-------------------------------|----------------------|--------|
| **`set_tag`** | Planned | Planned | Visible to later hooks — for example response rule `tag` selectors after `post_wait` |
| **`set_pool`** | Planned | — | Before [Route](/concepts/architecture-and-packet-path.md#route); may override pool from request rules |
| **Drop** | Planned | Planned | Same semantics as rule `drop` |
| **Retry** | — | Planned | Re-enters [Route](/concepts/architecture-and-packet-path.md#route) (egress only), like response-hook `retry` |

Rhai equivalents for these effects will be documented on [Message API](/rhai/message-api.md) when the feature ships.

## Shared host features

Lookup tables (`data_sources:`) and custom metrics are host-owned features shared across Rhai and planned WASM/sidecar backends. Reference: [Data sources and lookups](/rhai/data-sources-and-lookups.md), [User metrics](/rhai/user-metrics.md).

## Until this ships

Configuration fields, validation rules, and operator workflows will be documented here when processor chains land. Roadmap detail: [Planned plugin models](/concepts/planned-plugin-models.md#processor-chains-planned).

## Related

- [Rhai overview](/rhai/index.md) — Rhai for rules vs Rhai for processor chains
- [Policy & routing](/policy-routing/index.md) — rules and Rhai for rules today
- [Architecture and packet path](/concepts/architecture-and-packet-path.md) — pipeline phases
