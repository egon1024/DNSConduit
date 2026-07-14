# rules-drop-now

## Purpose

Confirm hard **`drop_now`** on a matching request rule: the client must receive
**no successful answer**. On the wire this looks like soft `drop`; on Conduit’s
action list, `drop_now` also short-circuits later actions on that rule.

Results appear on the **Conduit behavior** matrix (one stub peer).

## How it works

1. Conduit runs with a request rule: qnames under `.hard-drop.test.` get
   `drop_now` before the catch-all `set_pool`.
2. A client queries `probe.hard-drop.test` A.
3. Checks require the **no-answer** property.

## Outcomes

| Outcome | Meaning for operators |
|---|---|
| <span class="interop-outcome interop-outcome--pass">pass</span> | Hard-dropped query produced no successful client answer. |
| <span class="interop-outcome interop-outcome--fail">fail</span> | Unexpected: query still got a successful answer — `drop_now` not applied. |
| <span class="interop-outcome interop-outcome--skip">skip</span> | This peer/profile combination is out of scope. |
| <span class="interop-outcome interop-outcome--characterized">characterized</span> | Not used for this case today. |

**Matrix:** conduit ([Conduit behavior](/interop/conduit-behavior.md))

**Suites:** full

**Oracles:** property
