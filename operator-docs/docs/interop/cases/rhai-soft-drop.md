# rhai-soft-drop

## Purpose

Confirm a **Rhai** request-hook script can soft-drop a query (`txn.drop_query()`),
so the client gets **no successful answer**. This is the scripted analogue of
YAML `drop` / `drop_now`, used for blocklists and custom policy.

Results appear on the **Conduit behavior** matrix (one stub peer).

## How it works

1. Conduit loads a Rhai script and runs it from a request rule matching
   `.rhai-drop.test.`.
2. A client queries `probe.rhai-drop.test` A.
3. Checks require the **no-answer** property.

## Outcomes

| Outcome | Meaning for operators |
|---|---|
| <span class="interop-outcome interop-outcome--pass">pass</span> | Rhai soft-drop produced no successful client answer. |
| <span class="interop-outcome interop-outcome--fail">fail</span> | Unexpected: query still answered — script loading or `drop_query` path broken. |
| <span class="interop-outcome interop-outcome--skip">skip</span> | This peer/profile combination is out of scope. |
| <span class="interop-outcome interop-outcome--characterized">characterized</span> | Not used for this case today. |

**Matrix:** conduit ([Conduit behavior](/interop/conduit-behavior.md))

**Suites:** full

**Oracles:** property
