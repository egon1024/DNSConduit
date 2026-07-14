# rules-soft-drop

## Purpose

Confirm soft **`drop`** on a matching request rule: the client must receive **no
successful answer** (timeout / empty), not a normal forward response.

Results appear on the **Conduit behavior** matrix (one stub peer), because this
asserts Conduit policy, not peer-product interoperability.

## How it works

1. Conduit runs with a request rule: qnames under `.soft-drop.test.` get `drop`
   before the catch-all `set_pool`.
2. A client queries `probe.soft-drop.test` A (no peer answer is required for the
   drop path).
3. Checks require the **no-answer** property (TIMEOUT/UNKNOWN/empty — not NOERROR
   with RRs).

## Outcomes

| Outcome | Meaning for operators |
|---|---|
| <span class="interop-outcome interop-outcome--pass">pass</span> | Soft-dropped query produced no successful client answer. |
| <span class="interop-outcome interop-outcome--fail">fail</span> | Unexpected: query still got a successful answer — drop rule not applied. |
| <span class="interop-outcome interop-outcome--skip">skip</span> | This peer/profile combination is out of scope. |
| <span class="interop-outcome interop-outcome--characterized">characterized</span> | Not used for this case today. |

**Matrix:** conduit ([Conduit behavior](/interop/conduit-behavior.md))

**Suites:** full

**Oracles:** property
