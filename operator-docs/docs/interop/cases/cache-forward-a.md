# cache-forward-a

## Purpose

With a **cache → forward** lookup chain enabled, the **first** query for a
local smoke A (cold miss, then forward fill) must still succeed. Results are
on the **Conduit behavior** matrix (one stub peer): this is about Conduit’s
cache path, not peer-product differences.

A later warm-path miss→hit (including peer query-count proof) is checked by
[cache-miss-then-hit-a](/interop/cases/cache-miss-then-hit-a.md); this case
only covers the cold miss answer.

## How it works

1. The stub peer answers a local smoke name (`www.smoke.test` → `192.0.2.20`).
2. Conduit uses the **cache-forward** profile (in-memory cache in front of forward).
3. A client queries Conduit for that A name once.
4. The reply must be NOERROR with at least one answer record.

## Outcomes

| Outcome | Meaning for operators |
|---|---|
| <span class="interop-outcome interop-outcome--pass">pass</span> | Cache-enabled forward path returned a successful A answer for the smoke name. |
| <span class="interop-outcome interop-outcome--fail">fail</span> | No successful answer — investigate cache/lookup config, forwarding, or peer readiness. |
| <span class="interop-outcome interop-outcome--skip">skip</span> | This profile or peer is out of scope (cache-forward on the Conduit-behavior stub only). |
| <span class="interop-outcome interop-outcome--characterized">characterized</span> | Not used for this case today. If it appeared, it would mean a documented peer-specific quirk rather than a Conduit regression. |

**Matrix:** conduit ([Conduit behavior](/interop/conduit-behavior.md))

**Suites:** full

**Oracles:** property
