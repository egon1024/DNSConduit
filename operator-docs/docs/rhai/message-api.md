# Message API

**Not yet shipped.** Planned reference for **Rhai for processor chains** ([Processor-chain Rhai](/glossary/index.md#processor-chain-rhai)): the **`conduit-dns`** message surface for editing DNS wire bytes in [processor chains](/processor-chains/index.md).

## Planned scope

Processor-chain scripts receive a message object (exact Rhai binding name TBD at ship time) for the current query or response wire — not the rule **`txn`** policy object. Capabilities are expected to include:

- Ingress query changes (for example qname rewrite)
- Egress response mutation
- EDNS / EDE helpers
- Conveniences such as RD-bit control

Full method entries will follow the same card layout as [Transaction API](/rhai/transaction-api.md).

## Until this ships

- [Rhai for processor chains](/rhai/processor-chain-rhai.md) — Rhai backend overview
- [Processor chains](/processor-chains/index.md) — `processors:` config and pipeline placement
- [Rhai for rules](/rhai/rule-rhai.md) — policy-only scripting with `txn` (shipped today)

## Related

- [Rhai overview](/rhai/index.md)
