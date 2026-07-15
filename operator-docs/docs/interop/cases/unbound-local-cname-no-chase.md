# unbound-local-cname-no-chase

## Purpose

**Unbound** local-data can return a **CNAME without chasing** to a target A that
exists in the same local zone — clients often get CNAME alone and must follow
up themselves. When Conduit forwards to Unbound, operators should still see
that peer-native answer (Conduit does not synthesize the missing A).

## How it works

1. Unbound is configured with a static local zone holding both an alias CNAME
   and a separate local A for its target.
2. A client asks Conduit for the alias as type A.
3. The answer must contain CNAME only (no target A) and match a direct query
   to Unbound.

## Outcomes

| Outcome | Meaning for operators |
|---|---|
| <span class="interop-outcome interop-outcome--pass">pass</span> | Through Conduit, the alias returns the same CNAME-only answer as querying Unbound directly. |
| <span class="interop-outcome interop-outcome--fail">fail</span> | Conduit altered the answer relative to Unbound, or the reply was not CNAME-only. |
| <span class="interop-outcome interop-outcome--skip">skip</span> | Peer is not Unbound for this matrix (or profile out of scope). |
| <span class="interop-outcome interop-outcome--characterized">characterized</span> | Not used for this case today. If it appeared, it would mean a documented peer-specific quirk rather than a Conduit regression. |

**Matrix:** peer (by publisher)

**Suites:** full

**Oracles:** property, parity
