# cache-nxdomain-miss-then-hit

## Purpose

With **cache → forward** and **negative caching** enabled, two identical **A**
queries for a name the stub answers as **NXDOMAIN** both return NXDOMAIN. The
first query is a cold miss (forward fill); the second must also return NXDOMAIN.

Results appear on the **Conduit behavior** matrix (one stub peer).

This does **not** prove the second query skipped the peer.

## How it works

1. The stub treats `nxcache.test` as a local zone with no positive records, so
   `missing.nxcache.test` → NXDOMAIN.
2. Conduit uses the **cache-forward** profile (negative cache on).
3. The harness issues the same A query **twice**.
4. Each step must be **NXDOMAIN**.

## Outcomes

| Outcome | Meaning for operators |
|---|---|
| <span class="interop-outcome interop-outcome--pass">pass</span> | Both queries returned NXDOMAIN with negative caching configured. |
| <span class="interop-outcome interop-outcome--fail">fail</span> | Unexpected response code — investigate negative cache or forwarding. |
| <span class="interop-outcome interop-outcome--skip">skip</span> | This profile or peer is out of scope. |
| <span class="interop-outcome interop-outcome--characterized">characterized</span> | Not used for this case today. If it appeared, it would mean a documented peer-specific quirk rather than a Conduit regression. |

**Matrix:** conduit ([Conduit behavior](/interop/conduit-behavior.md))

**Suites:** full

**Oracles:** property
