# health-drain-avoids-backend

## Purpose

An operator **drain** (`conduitctl health set down`) removes a backend from
Route while a sibling stays Up, so client traffic goes only to the live
backend.

Results appear on the **Conduit behavior** matrix (one stub peer). Requires a
host **`conduitctl`** binary (see interop README).

## How it works

1. Pool: `live` (stub peer) and `dead` (unused IP); probes are slow so they
   do not race the drain proof.
2. Control plane listens on `0.0.0.0:5199` (published to the host).
3. The harness runs `conduitctl health set down --pool default --backend dead`,
   then digs `www.smoke.test`.
4. Checks require NOERROR, one peer query, and forward success only on `live`.

## Outcomes

| Outcome | Meaning for operators |
|---|---|
| <span class="interop-outcome interop-outcome--pass">pass</span> | After draining `dead`, traffic stayed on `live`. |
| <span class="interop-outcome interop-outcome--fail">fail</span> | Drain did not stick, digs failed, or forwards still hit `dead`. |
| <span class="interop-outcome interop-outcome--skip">skip</span> | This peer/profile combination is out of scope. |
| <span class="interop-outcome interop-outcome--characterized">characterized</span> | Not used for this case today. If it appeared, it would mean a documented peer-specific quirk rather than a Conduit regression. |

**Matrix:** conduit ([Conduit behavior](/interop/conduit-behavior.md))

**Suites:** full

**Oracles:** property, peer-query-count, metrics-delta
