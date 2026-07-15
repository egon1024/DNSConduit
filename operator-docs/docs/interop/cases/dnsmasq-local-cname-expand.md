# dnsmasq-local-cname-expand

## Purpose

When **dnsmasq** has a CNAME whose target is also known locally, it expands
the answer to **CNAME+A**. Through Conduit, clients should see that expanded
answer — Conduit does not drop the A or the CNAME.

## How it works

1. dnsmasq is configured with a local CNAME and a local A for its target.
2. A client asks Conduit for the alias as type A.
3. The answer must include both CNAME and A, matching a direct query to dnsmasq.

## Outcomes

| Outcome | Meaning for operators |
|---|---|
| <span class="interop-outcome interop-outcome--pass">pass</span> | Expanded local CNAME through Conduit matches dnsmasq. |
| <span class="interop-outcome interop-outcome--fail">fail</span> | Missing expansion, wrong rcode, or Conduit disagreed with dnsmasq. |
| <span class="interop-outcome interop-outcome--skip">skip</span> | Peer is not the dnsmasq stub under test (or profile out of scope). |
| <span class="interop-outcome interop-outcome--characterized">characterized</span> | Not used for this case today. If it appeared, it would mean a documented peer-specific quirk rather than a Conduit regression. |

**Matrix:** peer (by publisher)

**Suites:** full

**Oracles:** property, parity
