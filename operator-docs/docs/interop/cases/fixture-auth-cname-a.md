# fixture-auth-cname-a

## Purpose

Authoritative peers expand a fixture **CNAME** to the target **A** in the
answer. Through Conduit, clients should see that same CNAME+A chain — Conduit
does not drop the CNAME or invent a different address set.

## How it works

1. The peer loads `example.test`, including `alias CNAME www`.
2. A client asks Conduit for `alias.example.test` A.
3. The answer must match the committed fixture and a direct query to the peer.

## Outcomes

| Outcome | Meaning for operators |
|---|---|
| <span class="interop-outcome interop-outcome--pass">pass</span> | CNAME+A through Conduit matches the fixture and the peer. |
| <span class="interop-outcome interop-outcome--fail">fail</span> | Missing CNAME or A, wrong rcode, or Conduit disagreed with the peer. |
| <span class="interop-outcome interop-outcome--skip">skip</span> | Peer is not authoritative for this matrix (or profile out of scope). |
| <span class="interop-outcome interop-outcome--characterized">characterized</span> | Not used for this case today. If it appeared, it would mean a documented peer-specific quirk rather than a Conduit regression. |

**Matrix:** peer (by publisher)

**Suites:** full

**Oracles:** fixture, property, parity
