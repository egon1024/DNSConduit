# edns-opt-passthrough-smoke

## Purpose

Clients that send **EDNS** (Extension Mechanisms for DNS) expect replies that
still carry an EDNS OPT. On the default forward path, Conduit must not strip
that OPT from a successful peer answer.

## How it works

1. The peer answers a local smoke A name.
2. A client queries that name through Conduit using EDNS.
3. The reply must succeed with EDNS present, matching a direct query to the peer.

## Outcomes

| Outcome | Meaning for operators |
|---|---|
| <span class="interop-outcome interop-outcome--pass">pass</span> | Successful A through Conduit still includes EDNS, matching the peer. |
| <span class="interop-outcome interop-outcome--fail">fail</span> | EDNS missing via Conduit while present from the peer, or the answer otherwise disagreed. |
| <span class="interop-outcome interop-outcome--skip">skip</span> | This peer/profile combination is out of scope for the case. |
| <span class="interop-outcome interop-outcome--characterized">characterized</span> | Not used for this case today. If it appeared, it would mean a documented peer-specific quirk rather than a Conduit regression. |

**Matrix:** peer (by publisher)

**Suites:** full

**Oracles:** property, parity
