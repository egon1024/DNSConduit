# rules-clear-drop

## Purpose

Confirm **`clear_drop`**: a request rule may set soft `drop` and then cancel it
so the query still forwards and returns a successful **A** answer. Operators can
undo soft-drop intent on the same rule.

Results appear on the **Conduit behavior** matrix (one stub peer).

## How it works

1. The peer answers `allow.clear-drop.test` → `192.0.2.20`.
2. Conduit runs with a matching rule that applies `drop` then `clear_drop`, then
   the catch-all `set_pool`.
3. A client queries that name through Conduit.
4. Checks require **NOERROR** and at least one answer RR.

## Outcomes

| Outcome | Meaning for operators |
|---|---|
| <span class="interop-outcome interop-outcome--pass">pass</span> | Soft drop was cleared; the forward path returned a successful A answer. |
| <span class="interop-outcome interop-outcome--fail">fail</span> | Unexpected: still no answer or other failure — `clear_drop` did not restore forward. |
| <span class="interop-outcome interop-outcome--skip">skip</span> | This peer/profile combination is out of scope. |
| <span class="interop-outcome interop-outcome--characterized">characterized</span> | Not used for this case today. |

**Matrix:** conduit ([Conduit behavior](/interop/conduit-behavior.md))

**Suites:** full

**Oracles:** property
