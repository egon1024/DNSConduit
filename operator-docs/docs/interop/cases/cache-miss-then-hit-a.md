# cache-miss-then-hit-a

## Purpose

With **cache → forward** enabled, two identical **A** queries both return a
successful answer, the stub peer receives **exactly one** upstream query from
Conduit (cold miss fill; warm path skips the peer), the warm answer’s TTL is
**strictly lower** than the cold fill, and Conduit metrics record one cache
**miss** then one **hit** (with matching `answer_source` response labels).

Results appear on the **Conduit behavior** matrix (one stub peer).

## How it works

1. The stub peer answers `www.smoke.test` → `192.0.2.20` with a positive TTL.
2. Conduit uses the **cache-forward** profile (Prometheus scrape enabled).
3. The harness snapshots peer logs and `/metrics`, issues the A query via
   Conduit (cold miss), waits two seconds, issues the same query again
   (warm hit), then checks log and metrics deltas.
4. Each DNS step must be **NOERROR** with at least one answer RR; the peer
   must show **one** Conduit-sourced query; answer TTL must decrease; and
   metrics must show miss+forward then hit+cache increments of one each.

## Outcomes

| Outcome | Meaning for operators |
|---|---|
| <span class="interop-outcome interop-outcome--pass">pass</span> | Answers succeeded, peer saw one Conduit query, TTL decayed, and cache metrics recorded miss then hit. |
| <span class="interop-outcome interop-outcome--fail">fail</span> | DNS, peer-query, TTL, or metrics assertion failed — investigate the cache/forward path. |
| <span class="interop-outcome interop-outcome--skip">skip</span> | This profile or peer is out of scope (cache-forward on the Conduit-behavior stub only). |
| <span class="interop-outcome interop-outcome--characterized">characterized</span> | Not used for this case today. If it appeared, it would mean a documented peer-specific quirk rather than a Conduit regression. |

**Matrix:** conduit ([Conduit behavior](/interop/conduit-behavior.md))

**Suites:** full

**Oracles:** property, peer-query-count, sequence, metrics-delta
