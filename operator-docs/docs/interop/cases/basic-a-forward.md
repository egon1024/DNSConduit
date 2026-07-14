# basic-a-forward

## Purpose

Confirm the minimal forward path: a client asks Conduit for a simple **A**
record, Conduit forwards to the peer under test, and the client gets a
successful answer. This is the baseline proxy contract for every supported
peer role (stub, authoritative, and recursive).

## How it works

1. For this run, the peer is configured with a local smoke name
   (`www.smoke.test` → `192.0.2.20`) so the answer does not depend on the
   public Internet.
2. A client queries Conduit (forward-only profile); Conduit forwards to that
   peer.
3. Checks require **NOERROR** and at least one answer section RR.
4. A **parity** check also queries the peer directly and compares response
   code and answer count with the via-Conduit result — Conduit should not
   invent or drop success/failure relative to talking to the peer alone.

## Outcomes

| Outcome | Meaning for operators |
|---|---|
| <span class="interop-outcome interop-outcome--pass">pass</span> | Conduit returned a successful A answer for the smoke name, and that success matches querying the peer directly (same rcode and answer count). |
| <span class="interop-outcome interop-outcome--fail">fail</span> | Unexpected: Conduit failed the query, returned no answer, or disagreed with a direct query to the peer. Investigate Conduit forwarding, the peer image/config for this run, or a peer version regression. |
| <span class="interop-outcome interop-outcome--skip">skip</span> | This peer/profile combination is out of scope for the case (not applicable). |
| <span class="interop-outcome interop-outcome--characterized">characterized</span> | Not used for this case today. If it appeared, it would mean a documented peer-specific quirk rather than a Conduit regression. |

**Suites:** smoke, full

**Oracles:** property, parity
