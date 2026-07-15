# cache-nxdomain-miss-then-hit

## Purpose

With **cache → forward** and **negative caching** enabled, two identical **A**
queries for a name the stub answers as **NXDOMAIN** both return NXDOMAIN, the
stub peer receives **exactly one** upstream query from Conduit, and Conduit
metrics record one cache **miss** then one **hit**.

Results appear on the **Conduit behavior** matrix (one stub peer).

## How it works

1. The stub treats `nxcache.test` as a local zone with no positive records, so
   `missing.nxcache.test` → NXDOMAIN.
2. Conduit uses the **cache-forward** profile (negative cache on; Prometheus
   scrape enabled).
3. The harness snapshots peer logs and `/metrics`, issues the same A query
   **twice** via Conduit, then checks log and metrics deltas.
4. Each DNS step must be **NXDOMAIN**; the peer must show **one**
   Conduit-sourced query; and metrics must show miss+forward then hit+cache
   increments of one each.

## Outcomes

| Outcome | Meaning for operators |
|---|---|
| <span class="interop-outcome interop-outcome--pass">pass</span> | Both answers were NXDOMAIN, the peer saw only one Conduit query, and cache metrics recorded miss then hit. |
| <span class="interop-outcome interop-outcome--fail">fail</span> | Unexpected response code, peer was queried more than once, or metrics did not show miss then hit — investigate negative cache or forwarding. |
| <span class="interop-outcome interop-outcome--skip">skip</span> | This profile or peer is out of scope. |
| <span class="interop-outcome interop-outcome--characterized">characterized</span> | Not used for this case today. If it appeared, it would mean a documented peer-specific quirk rather than a Conduit regression. |

**Matrix:** conduit ([Conduit behavior](/interop/conduit-behavior.md))

**Suites:** full

**Oracles:** property, peer-query-count, metrics-delta
