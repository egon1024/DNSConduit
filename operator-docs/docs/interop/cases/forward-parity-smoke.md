# forward-parity-smoke

## Purpose

Smoke check for the forward happy path: on the **forward-only** profile, Conduit
returns **NOERROR** with an answer for a simple **A** query.

Compared with [`basic-a-forward`](basic-a-forward.md), this case checks only
those response **properties**. It does **not** compare the via-Conduit answer to
a direct query at the peer.

## How it works

1. The peer is configured with a local smoke name (`www.smoke.test` →
   `192.0.2.20`) so the answer does not depend on the public Internet.
2. A client queries Conduit; Conduit forwards to the peer.
3. Checks require **NOERROR** and at least one answer RR.

## Outcomes

| Outcome | Meaning for operators |
|---|---|
| <span class="interop-outcome interop-outcome--pass">pass</span> | Conduit returned NOERROR with an answer for the smoke A query. |
| <span class="interop-outcome interop-outcome--fail">fail</span> | Unexpected: no successful answer through Conduit. Investigate forwarding, peer readiness for this run, or a peer version change that broke the smoke name. |
| <span class="interop-outcome interop-outcome--skip">skip</span> | This peer/profile combination is out of scope for the case. |
| <span class="interop-outcome interop-outcome--characterized">characterized</span> | Not used for this case today. |

**Matrix:** peer (by publisher)

**Suites:** smoke, full

**Oracles:** property
