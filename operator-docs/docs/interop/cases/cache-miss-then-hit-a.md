# cache-miss-then-hit-a

## Purpose

With **cache → forward** enabled, two identical **A** queries both return a
successful answer. The first query is a cold miss (forward fill); the second
must also succeed on the warm path.

Results appear on the **Conduit behavior** matrix (one stub peer).

This does **not** prove the second query skipped the peer (that needs metrics or
a peer query counter). It only proves two successful answers with caching
configured.

## How it works

1. The stub peer answers `www.smoke.test` → `192.0.2.20`.
2. Conduit uses the **cache-forward** profile.
3. The harness issues the same A query **twice**.
4. Each step must be **NOERROR** with at least one answer RR.

## Outcomes

| Outcome | Meaning for operators |
|---|---|
| <span class="interop-outcome interop-outcome--pass">pass</span> | Both queries returned successful A answers with caching enabled. |
| <span class="interop-outcome interop-outcome--fail">fail</span> | Unexpected: the first or second query failed — investigate the cache/forward path. |
| <span class="interop-outcome interop-outcome--skip">skip</span> | This profile or peer is out of scope (cache-forward on the Conduit-behavior stub only). |
| <span class="interop-outcome interop-outcome--characterized">characterized</span> | Not used for this case today. |

**Matrix:** conduit ([Conduit behavior](/interop/conduit-behavior.md))

**Suites:** full

**Oracles:** property
