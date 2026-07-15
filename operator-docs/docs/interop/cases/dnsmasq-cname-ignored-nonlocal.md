# dnsmasq-cname-ignored-nonlocal

## Purpose

With **dnsmasq**, a configured CNAME whose target is **not** known locally is
ignored. Asking for the alias can return **NXDOMAIN** even though a neighboring
local A name still works. When Conduit sits in front of dnsmasq, clients should
see that same NXDOMAIN — Conduit does not invent an expansion dnsmasq itself
would not provide.

## How it works

1. dnsmasq is set up with a CNAME pointing at a non-local name, a control local
   A, and a local zone so unanswered names under that zone stay local.
2. A client queries the alias as type A through Conduit.
3. The reply must be NXDOMAIN and match a direct query to dnsmasq.

## Outcomes

| Outcome | Meaning for operators |
|---|---|
| <span class="interop-outcome interop-outcome--pass">pass</span> | Not expected for this case; a matching result is reported as characterized. |
| <span class="interop-outcome interop-outcome--fail">fail</span> | Through Conduit the alias was unexpectedly answered, or the reply disagreed with querying dnsmasq directly. |
| <span class="interop-outcome interop-outcome--skip">skip</span> | Peer is not the dnsmasq stub under test (or profile out of scope). |
| <span class="interop-outcome interop-outcome--characterized">characterized</span> | Expected: NXDOMAIN for the ignored nonlocal CNAME through Conduit, matching dnsmasq. |

**Matrix:** peer (by publisher)

**Suites:** full

**Oracles:** property, parity, differential
