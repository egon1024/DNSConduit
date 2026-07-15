# fixture-auth-aaaa

## Purpose

Confirm that when Conduit forwards to an **authoritative** peer serving a
published fixture zone, a published **AAAA** answer matches the committed
fixture for `www.example.test`.

Applies only to peers in the **auth** role.

## How it works

1. The peer loads fixture zone `example.test` (includes `www` AAAA).
2. A client asks Conduit for `www.example.test` AAAA.
3. The response must match the committed fixture.

## Outcomes

| Outcome | Meaning for operators |
|---|---|
| <span class="interop-outcome interop-outcome--pass">pass</span> | AAAA through Conduit matches the published fixture. |
| <span class="interop-outcome interop-outcome--fail">fail</span> | Unexpected response code or answers — zone contents, auth peer, or Conduit path. |
| <span class="interop-outcome interop-outcome--skip">skip</span> | Peer is not authoritative (or profile out of scope). |
| <span class="interop-outcome interop-outcome--characterized">characterized</span> | Not used for this case today. If it appeared, it would mean a documented peer-specific quirk rather than a Conduit regression. |

**Matrix:** peer (by publisher)

**Suites:** full

**Oracles:** fixture
