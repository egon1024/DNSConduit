# dataplane-split-io-forward

## Purpose

Confirm that with **`dataplane.runtime: split_io`**, Conduit still forwards a
simple **A** query to the peer and returns a successful answer. Split I/O is a
Conduit runtime choice; it must not break the basic forward contract.

Results appear on the **Conduit behavior** matrix (one stub peer).

## How it works

1. The stub peer answers `www.smoke.test` → `192.0.2.20`.
2. Conduit uses the **forward-split-io** profile.
3. A client queries Conduit once for that A name.
4. Checks require **NOERROR** and at least one answer RR.

## Outcomes

| Outcome | Meaning for operators |
|---|---|
| <span class="interop-outcome interop-outcome--pass">pass</span> | Split I/O dataplane still delivered a successful forward A answer. |
| <span class="interop-outcome interop-outcome--fail">fail</span> | Unexpected: no successful answer — investigate dataplane runtime or peer readiness. |
| <span class="interop-outcome interop-outcome--skip">skip</span> | This profile or peer is out of scope (forward-split-io on the Conduit-behavior stub only). |
| <span class="interop-outcome interop-outcome--characterized">characterized</span> | Not used for this case today. |

**Matrix:** conduit ([Conduit behavior](/interop/conduit-behavior.md))

**Suites:** full

**Oracles:** property
