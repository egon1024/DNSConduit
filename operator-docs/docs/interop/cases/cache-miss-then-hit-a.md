# cache-miss-then-hit-a

## Purpose

With **cache → forward** enabled, two identical **A** queries both return a
successful answer, and the stub peer receives **exactly one** upstream query
from Conduit (cold miss fill; warm path must skip the peer).

Results appear on the **Conduit behavior** matrix (one stub peer).

## How it works

1. The stub peer answers `www.smoke.test` → `192.0.2.20`.
2. Conduit uses the **cache-forward** profile.
3. The harness snapshots the peer’s query log, issues the same A query
   **twice** via Conduit, then checks the log delta.
4. Each DNS step must be **NOERROR** with at least one answer RR, and the
   peer must show **one** Conduit-sourced query for that name/type.

## Outcomes

| Outcome | Meaning for operators |
|---|---|
| <span class="interop-outcome interop-outcome--pass">pass</span> | Both client answers succeeded and the peer saw only one Conduit query (cache hit skipped upstream). |
| <span class="interop-outcome interop-outcome--fail">fail</span> | A client answer failed, or the peer was queried more than once — investigate the cache/forward path. |
| <span class="interop-outcome interop-outcome--skip">skip</span> | This profile or peer is out of scope (cache-forward on the Conduit-behavior stub only). |
| <span class="interop-outcome interop-outcome--characterized">characterized</span> | Not used for this case today. If it appeared, it would mean a documented peer-specific quirk rather than a Conduit regression. |

**Matrix:** conduit ([Conduit behavior](/interop/conduit-behavior.md))

**Suites:** full

**Oracles:** property, peer-query-count
