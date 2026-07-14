# fixture-auth-nxdomain

## Purpose

Confirm that when Conduit forwards to an **authoritative** peer serving a
published fixture zone, a name that is **in zone but missing** returns the
committed **NXDOMAIN** outcome. Complements
[`fixture-auth-a`](fixture-auth-a.md) (positive A answers).

Applies only to peers in the **auth** role.

## How it works

1. The peer loads fixture zone `example.test`.
2. A client asks Conduit for `missing.example.test` A (not present in the zone).
3. The response must match the committed fixture (`rcode: NXDOMAIN`, no answers).

## Outcomes

| Outcome | Meaning for operators |
|---|---|
| <span class="interop-outcome interop-outcome--pass">pass</span> | NXDOMAIN through Conduit matches the published fixture for the missing name. |
| <span class="interop-outcome interop-outcome--fail">fail</span> | Unexpected response code or answers — zone contents, auth peer behavior, or Conduit error handling. |
| <span class="interop-outcome interop-outcome--skip">skip</span> | Peer is not authoritative (or profile out of scope). Expected for recursive/stub columns. |
| <span class="interop-outcome interop-outcome--characterized">characterized</span> | Not used for this case today. |

**Matrix:** peer (by publisher)

**Suites:** full

**Oracles:** fixture
