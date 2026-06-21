# Dual-stack forwarding

This guide covers **upstream egress** — which local address Conduit uses when forwarding to [backends](/glossary/index.md#backend). Config fields: [Reference: forward](/reference/config-schema/forward.md), [Reference: pools](/reference/config-schema/pools.md). For pool and backend layout, see [Pools and backends](/policy-routing/pools-and-backends.md). For the query path, see [Architecture and packet path](/concepts/architecture-and-packet-path.md).

## Global and per-pool sources

Configure local bind addresses under:

- **`forward.sources_v4`** / **`forward.sources_v6`** — defaults for all pools
- **`pools[].sources_v4`** / **`pools[].sources_v6`** — override for a specific [pool](/glossary/index.md#pool) when non-empty

When a pool list is empty, Conduit uses the global `forward.sources_*` list. See [Reference: pools](/reference/config-schema/pools.md) for limits and validation.

At [Forward](/concepts/architecture-and-packet-path.md#forward), Conduit picks a source using **`forward.source_selection`** (round-robin in current releases) unless a per-query override is set.

## Choosing an egress source {#choosing-an-egress-source}

Use this order of preference:

| Need | Mechanism |
|------|-----------|
| **Same source for every query to a pool** | Pool `sources_v4` / `sources_v6` (or global `forward.sources_*`) |
| **Source depends on query match, fixed IP** | [Request rules](/concepts/architecture-and-packet-path.md#request-rules) — `set_source_v4` / `set_source_v6` ([Rules and actions](/policy-routing/rules-and-actions.md)) |
| **Different egress only on retry (outcome-driven)** | Request or [response rules](/policy-routing/rules-and-actions.md) — `set_retry_source_v4` / `set_retry_source_v6`; pair with `retry` / `retry_now` or Rhai `txn.request_retry()` |
| **Logic, tables, or multi-step policy** | [Rhai](/rhai/index.md) — `txn.set_source_v4()` / `txn.set_source_v6()` on the request hook; `txn.set_retry_source_*()` on either hook |

Declarative **`set_source_*`** actions and [Rhai](/rhai/index.md) share the same [transaction](/glossary/index.md#transaction) overrides and the same **allowed-set** check at [Forward](/concepts/architecture-and-packet-path.md#forward). If an override is not permitted for the selected pool, Conduit **does not fail the query** — it falls back to ordinary round-robin among configured sources.

When a rule sets both pool and source, list **`set_pool` before `set_source_v4` / `set_source_v6`** on the same rule. Details: [Action order on one rule](/policy-routing/rules-and-actions.md#action-order-on-one-rule).

## IPv4 clients, IPv6 backends (and the reverse)

Conduit selects the source address family to match the [backend](/glossary/index.md#backend) address (IPv4 backend → v4 source list; IPv6 backend → v6 source list). Cross-family forwarding is determined by your pool/backend layout and source lists — validate end-to-end in a lab before production.

Manual lab scenarios: `tests/manual/ipv4-ipv6-forwarding.md` in the product repository (not linked from published docs).

## Related topics

- [Rules and actions](/policy-routing/rules-and-actions.md) — `set_source_v4`, `set_source_v6`
- [Rhai transaction API](/rhai/transaction-api.md) — script equivalents
- [Policy & routing](/policy-routing/index.md) — rules and scripted policy hooks
