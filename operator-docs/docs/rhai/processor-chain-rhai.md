# Processor-chain Rhai

**Not yet shipped.** **Rhai for processor chains** — [Processor-chain Rhai](/glossary/index.md#processor-chain-rhai) — will document Rhai as a backend inside [processor chains](/processor-chains/index.md): `.rhai` files referenced from `processors:` config that edit DNS wire bytes and related message metadata.

## Planned scope

Processor-chain Rhai uses the **message API** ([Message API](/rhai/message-api.md), crate surface `conduit-dns`) — not the rule **`txn`** object. It is separate from [Rhai for rules](/rhai/rule-rhai.md) ([Rule Rhai](/glossary/index.md#rule-rhai): `rules:` → `type: rhai`, [Transaction API](/rhai/transaction-api.md)).

When processor chains ship, this page will cover:

- How Rhai links are declared in `processors:` and ordered in a chain
- Pipeline attachment points (for example after [Request rules](/concepts/architecture-and-packet-path.md#request-rules), after upstream response)
- Phase rules for ingress vs egress scripts
- Sandbox limits specific to processor-chain hooks (may differ from Rule Rhai `rhai:` defaults)
- Policy refinement on the shared [transaction](/glossary/index.md#transaction) where applicable (`set_tag`, ingress `set_pool`, drop, egress retry) — see [Processor chains — policy refinement](/processor-chains/index.md#policy-refinement-planned)

## Until this ships

- Feature overview and multi-backend model: [Processor chains](/processor-chains/index.md), [Planned plugin models](/concepts/planned-plugin-models.md#processor-chains-planned)
- Rule-side policy scripting today: [Rhai for rules](/rhai/rule-rhai.md)

## Related

- [Rhai overview](/rhai/index.md)
- [Message API](/rhai/message-api.md) — planned wire-editing reference (stub)
- [Data sources and lookups](/rhai/data-sources-and-lookups.md) — shared `table_lookup` host API
