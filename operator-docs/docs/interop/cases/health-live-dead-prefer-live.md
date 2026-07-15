# health-live-dead-prefer-live

## Purpose

With **pool health** enabled and one live plus one dead backend, after probes
mark the dead backend down, client queries succeed via the live backend only.

Results appear on the **Conduit behavior** matrix (one stub peer).

## How it works

1. The stub peer answers `www.smoke.test` at `172.30.97.10:53` (named `live`).
2. A second pool member (`dead` at `172.30.97.99:53`) has nothing listening.
3. Health probes run with a short interval/`fall: 1`; the case sleeps so dead
   is applied-down before the client dig.
4. Checks require NOERROR with an answer, exactly one Conduit-sourced peer
   query for the name, and `conduit_forward_attempts_total` increments only on
   `backend=live` (not `dead`).

## Outcomes

| Outcome | Meaning for operators |
|---|---|
| <span class="interop-outcome interop-outcome--pass">pass</span> | After probe fall, traffic stayed on the live backend. |
| <span class="interop-outcome interop-outcome--fail">fail</span> | Client failed, peer saw unexpected query counts, or forwards hit `dead`. |
| <span class="interop-outcome interop-outcome--skip">skip</span> | This peer/profile combination is out of scope. |
| <span class="interop-outcome interop-outcome--characterized">characterized</span> | Not used for this case today. If it appeared, it would mean a documented peer-specific quirk rather than a Conduit regression. |

**Matrix:** conduit ([Conduit behavior](/interop/conduit-behavior.md))

**Suites:** full

**Oracles:** property, peer-query-count, metrics-delta
