# health-passive-fast-trip

## Purpose

With **passive fast-trip** enabled, the first client forward timeout to a dead
backend marks that backend down so a following query succeeds on the live
backend — without waiting for active probe `fall`.

Results appear on the **Conduit behavior** matrix (one stub peer).

## How it works

1. Pool members: `live` (stub peer) weight **1**, `dead` (unused IP) weight
   **10000** so Route almost always picks dead first while both are Up.
2. Active probes are intentionally slow (`fall: 50`) so they cannot win the
   race against the client digs; `passive_fall: 1` and `max_attempts: 1`.
3. Dig 1 times out against `dead` → SERVFAIL and passive trip; dig 2 goes to
   `live` → NOERROR.
4. Checks require step rcodes SERVFAIL then NOERROR, one peer query, and
   forward metrics showing one dead timeout plus one live success.

## Outcomes

| Outcome | Meaning for operators |
|---|---|
| <span class="interop-outcome interop-outcome--pass">pass</span> | Passive trip removed the dead backend; the next dig succeeded on live. |
| <span class="interop-outcome interop-outcome--fail">fail</span> | Dead was not tripped by the client timeout, or live never answered. |
| <span class="interop-outcome interop-outcome--skip">skip</span> | This peer/profile combination is out of scope. |
| <span class="interop-outcome interop-outcome--characterized">characterized</span> | Not used for this case today. If it appeared, it would mean a documented peer-specific quirk rather than a Conduit regression. |

**Matrix:** conduit ([Conduit behavior](/interop/conduit-behavior.md))

**Suites:** full

**Oracles:** sequence, property, peer-query-count, metrics-delta
