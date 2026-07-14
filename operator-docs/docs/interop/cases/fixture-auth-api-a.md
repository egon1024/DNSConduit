# fixture-auth-api-a

## Purpose

Confirm a second **positive A** name in the published fixture zone
(`api.example.test`) answers correctly through Conduit. Complements
[`fixture-auth-a`](fixture-auth-a.md) (`www`).

Applies only to peers in the **auth** role.

## How it works

1. The peer loads fixture zone `example.test`.
2. A client asks Conduit for `api.example.test` A.
3. The response must match the committed fixture.

## Outcomes

| Outcome | Meaning for operators |
|---|---|
| <span class="interop-outcome interop-outcome--pass">pass</span> | The second-name A through Conduit matches the published fixture. |
| <span class="interop-outcome interop-outcome--fail">fail</span> | Unexpected response code or answers — zone contents, auth peer, or Conduit path. |
| <span class="interop-outcome interop-outcome--skip">skip</span> | Peer is not authoritative (or profile out of scope). |
| <span class="interop-outcome interop-outcome--characterized">characterized</span> | Not used for this case today. |

**Matrix:** peer (by publisher)

**Suites:** full

**Oracles:** fixture
