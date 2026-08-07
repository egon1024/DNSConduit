# cache-lmdb-durability-restart-hit

## Purpose

With an **LMDB** answer cache in front of forward, a cold **A** fill stores the
answer on disk; after **restarting Conduit** (peer unchanged), the same query
returns successfully from cache and the stub peer receives **exactly one**
Conduit-sourced upstream query for that name (no second query after restart).

Results appear on the **Conduit behavior** matrix (one stub peer). This case
does **not** expand publisher peer-matrix rows.

## How it works

1. The stub peer answers `lmdb-durable.smoke.test` → `192.0.2.55` with a long TTL.
2. Conduit uses the **cache-forward** profile with a `conduit_delta` that swaps
   the named cache to `type: lmdb` on a writable `/var/lib/conduit` volume.
3. The harness issues the A query via Conduit (cold miss → fill), restarts only
   the Conduit container (peer and LMDB files remain), then issues the same
   query again (warm hit from durable store).
4. Each DNS step must be **NOERROR** with at least one answer RR; the peer must
   show **one** Conduit-sourced query across the whole sequence.

## Outcomes

| Outcome | Meaning for operators |
|---|---|
| <span class="interop-outcome interop-outcome--pass">pass</span> | Answers succeeded before and after restart, and the peer saw only one Conduit query — LMDB durability held. |
| <span class="interop-outcome interop-outcome--fail">fail</span> | DNS failed or the peer saw a second query after restart — cache did not survive or fill/hit path broke. |
| <span class="interop-outcome interop-outcome--skip">skip</span> | This profile or peer is out of scope (cache-forward on the Conduit-behavior stub only). |
| <span class="interop-outcome interop-outcome--characterized">characterized</span> | Not used for this case today. If it appeared, it would mean a documented peer-specific quirk rather than a Conduit regression. |

**Matrix:** conduit ([Conduit behavior](/interop/conduit-behavior.md))

**Suites:** full

**Oracles:** property, peer-query-count
