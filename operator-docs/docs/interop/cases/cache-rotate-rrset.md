# cache-rotate-rrset

## Purpose

Confirm **`rotate_rrset_on_serve`**: when a multi-A RRset is cached, warm answers
keep the same address membership while **answer order may change** across queries.

Results appear on the **Conduit behavior** matrix (one stub peer).

## How it works

1. The stub peer answers `rotate.smoke.test` with three A records.
2. Conduit uses **cache-forward** with `rotate_rrset_on_serve` enabled.
3. The harness queries the name several times (cold fill, then warm serves).
4. Each answer must be NOERROR with the expected address **set**; across the run,
   answer **order must vary** at least once.

## Outcomes

| Outcome | Meaning for operators |
|---|---|
| <span class="interop-outcome interop-outcome--pass">pass</span> | RRset membership held and at least two distinct answer orders were observed. |
| <span class="interop-outcome interop-outcome--fail">fail</span> | Membership was wrong, or order never varied — investigate rotate_on_serve or the peer RRset. |
| <span class="interop-outcome interop-outcome--skip">skip</span> | This profile or peer is out of scope. |
| <span class="interop-outcome interop-outcome--characterized">characterized</span> | Not used for this case today. If it appeared, it would mean a documented peer-specific quirk rather than a Conduit regression. |

**Matrix:** conduit ([Conduit behavior](/interop/conduit-behavior.md))

**Suites:** full

**Oracles:** property, sequence
