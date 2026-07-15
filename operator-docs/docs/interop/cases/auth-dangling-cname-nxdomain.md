# auth-dangling-cname-nxdomain

## Purpose

Authoritative peers often answer a **dangling CNAME** (CNAME present, target
missing in-zone) as **NXDOMAIN** while still including the CNAME in the answer
section. With Conduit in front, clients should see that same shape — Conduit
does not strip the CNAME or turn the reply into a plain NODATA/empty answer.

## How it works

1. The authoritative peer serves a zone with only `dangle CNAME gone` (no
   target address record).
2. A client asks Conduit for that alias as type A.
3. The reply must be NXDOMAIN with a CNAME in the answer, matching what the
   peer returns if queried directly.

## Outcomes

| Outcome | Meaning for operators |
|---|---|
| <span class="interop-outcome interop-outcome--pass">pass</span> | Through Conduit, dangling CNAME yields NXDOMAIN with the CNAME still answered, matching the auth peer. |
| <span class="interop-outcome interop-outcome--fail">fail</span> | Wrong rcode, missing CNAME, or Conduit’s answer disagreed with querying the peer directly. |
| <span class="interop-outcome interop-outcome--skip">skip</span> | Peer is not authoritative for this matrix (or profile out of scope). |
| <span class="interop-outcome interop-outcome--characterized">characterized</span> | Not used for this case today. If it appeared, it would mean a documented peer-specific quirk rather than a Conduit regression. |

**Matrix:** peer (by publisher)

**Suites:** full

**Oracles:** property, parity
