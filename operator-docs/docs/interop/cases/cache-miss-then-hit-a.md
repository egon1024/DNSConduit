# cache-miss-then-hit-a

## Purpose

With **cache → forward** enabled, two identical **A** queries both return a
successful answer, the stub peer receives **exactly one** upstream query from
Conduit (cold miss fill; warm path skips the peer), and the warm answer’s TTL
is **strictly lower** than the cold fill (serve-time TTL decay).

Results appear on the **Conduit behavior** matrix (one stub peer).

## How it works

1. The stub peer answers `www.smoke.test` → `192.0.2.20` with a positive TTL.
2. Conduit uses the **cache-forward** profile.
3. The harness snapshots the peer’s query log, issues the A query via
   Conduit (cold miss), waits two seconds, then issues the same query again
   (warm hit), and checks the peer log delta.
4. Each DNS step must be **NOERROR** with at least one answer RR; the peer
   must show **one** Conduit-sourced query; and the answer TTL must decrease
   from step 1 to step 2.

## Outcomes

| Outcome | Meaning for operators |
|---|---|
| <span class="interop-outcome interop-outcome--pass">pass</span> | Both answers succeeded, the peer saw only one Conduit query, and the warm TTL decayed. |
| <span class="interop-outcome interop-outcome--fail">fail</span> | A client answer failed, the peer was queried more than once, or TTL did not decay — investigate the cache/forward path. |
| <span class="interop-outcome interop-outcome--skip">skip</span> | This profile or peer is out of scope (cache-forward on the Conduit-behavior stub only). |
| <span class="interop-outcome interop-outcome--characterized">characterized</span> | Not used for this case today. If it appeared, it would mean a documented peer-specific quirk rather than a Conduit regression. |

**Matrix:** conduit ([Conduit behavior](/interop/conduit-behavior.md))

**Suites:** full

**Oracles:** property, peer-query-count, sequence
