# recursive-outside-aa-shape

## Purpose

With recursive peers configured for **local answers only** (no public
recursion), a query for a name **outside** those local zones typically returns
**NXDOMAIN** — but products disagree on the **AA** flag: **Unbound** sets AA
on that NXDOMAIN; **PowerDNS Recursor** does not. Through Conduit, clients
should still see whichever AA bit the backend produced.

## How it works

1. The peer is given one local A name only (no root hints or forwarders).
2. A client asks Conduit for a name in a different zone.
3. The reply must be NXDOMAIN with the same AA bit as a direct query to that
   peer.

## Outcomes

| Outcome | Meaning for operators |
|---|---|
| <span class="interop-outcome interop-outcome--pass">pass</span> | Not expected for this case; a matching result is reported as characterized. |
| <span class="interop-outcome interop-outcome--fail">fail</span> | Conduit changed the NXDOMAIN or AA bit relative to querying the peer directly. |
| <span class="interop-outcome interop-outcome--skip">skip</span> | Peer is not Unbound or PowerDNS Recursor for this case (or profile out of scope). |
| <span class="interop-outcome interop-outcome--characterized">characterized</span> | Expected: NXDOMAIN for the outside name through Conduit, with that peer’s AA bit (set for Unbound, clear for Recursor). |

**Matrix:** peer (by publisher)

**Suites:** full

**Oracles:** property, parity, differential
