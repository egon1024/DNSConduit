# cache-nxdomain-miss-then-hit

## Purpose

With **cache → forward** and **negative caching** enabled, two identical **A**
queries for a name the stub answers as **NXDOMAIN** both return NXDOMAIN, and
the stub peer receives **exactly one** upstream query from Conduit.

Results appear on the **Conduit behavior** matrix (one stub peer).

## How it works

1. The stub treats `nxcache.test` as a local zone with no positive records, so
   `missing.nxcache.test` → NXDOMAIN.
2. Conduit uses the **cache-forward** profile (negative cache on).
3. The harness snapshots the peer’s query log, issues the same A query
   **twice** via Conduit, then checks the log delta.
4. Each DNS step must be **NXDOMAIN**, and the peer must show **one**
   Conduit-sourced query for that name/type.

## Outcomes

| Outcome | Meaning for operators |
|---|---|
| <span class="interop-outcome interop-outcome--pass">pass</span> | Both answers were NXDOMAIN and the peer saw only one Conduit query (negative cache hit skipped upstream). |
| <span class="interop-outcome interop-outcome--fail">fail</span> | Unexpected response code, or the peer was queried more than once — investigate negative cache or forwarding. |
| <span class="interop-outcome interop-outcome--skip">skip</span> | This profile or peer is out of scope. |
| <span class="interop-outcome interop-outcome--characterized">characterized</span> | Not used for this case today. If it appeared, it would mean a documented peer-specific quirk rather than a Conduit regression. |

**Matrix:** conduit ([Conduit behavior](/interop/conduit-behavior.md))

**Suites:** full

**Oracles:** property, peer-query-count
