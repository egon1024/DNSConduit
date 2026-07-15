# stub-aaaa-for-a-only

## Purpose

When **dnsmasq** has a **local A** for a name but no AAAA, an AAAA query can
return a successful empty answer (NODATA-style) rather than following some
other product’s habit. Through Conduit, clients should get that same rcode and
empty answer Conduit does not invent AAAA or change the rcode.

## How it works

1. dnsmasq serves one A for the name and treats the parent as local so other
   types are not forwarded away.
2. A client asks Conduit for AAAA on that name.
3. The reply must match querying dnsmasq directly (NOERROR, no answer records
   in this lab).

## Outcomes

| Outcome | Meaning for operators |
|---|---|
| <span class="interop-outcome interop-outcome--pass">pass</span> | Not expected for this case; a matching result is reported as characterized. |
| <span class="interop-outcome interop-outcome--fail">fail</span> | Through Conduit the AAAA reply disagreed with querying dnsmasq directly. |
| <span class="interop-outcome interop-outcome--skip">skip</span> | Peer is not the dnsmasq stub under test (or profile out of scope). |
| <span class="interop-outcome interop-outcome--characterized">characterized</span> | Expected: AAAA on an A-only local name returns NOERROR with no answers through Conduit, matching dnsmasq. |

**Matrix:** peer (by publisher)

**Suites:** full

**Oracles:** parity, differential
