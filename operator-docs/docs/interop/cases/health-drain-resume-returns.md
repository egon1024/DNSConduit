# health-drain-resume-returns

## Purpose

Draining the only answering backend yields a forward-timeout **SERVFAIL**;
**resume** on that backend (with the sibling held down) restores successful
answers. Proves drain/resume round-trip without relying on fail-open alone.

Results appear on the **Conduit behavior** matrix (one stub peer). Requires a
host **`conduitctl`** binary (see interop README).

## How it works

1. Pool: `live` (stub) and `dead` (unused IP); slow probes so operator
   controls own applied state for the proof.
2. Drain `live` → dig gets SERVFAIL (only `dead` remains eligible and times
   out).
3. Resume `live`, then drain `dead` so fail-open cannot re-admit a blackhole.
4. Second dig succeeds on `live`.
5. Checks require SERVFAIL then NOERROR, one peer query overall, one dead
   timeout attempt, and one live success.

## Outcomes

| Outcome | Meaning for operators |
|---|---|
| <span class="interop-outcome interop-outcome--pass">pass</span> | Drain caused SERVFAIL; resume restored live answers. |
| <span class="interop-outcome interop-outcome--fail">fail</span> | Unexpected rcodes, peer traffic, or missing drain/resume effect. |
| <span class="interop-outcome interop-outcome--skip">skip</span> | This peer/profile combination is out of scope. |
| <span class="interop-outcome interop-outcome--characterized">characterized</span> | Not used for this case today. If it appeared, it would mean a documented peer-specific quirk rather than a Conduit regression. |

**Matrix:** conduit ([Conduit behavior](/interop/conduit-behavior.md))

**Suites:** full

**Oracles:** sequence, property, peer-query-count, metrics-delta
