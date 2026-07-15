# health-fail-open-all-down

## Purpose

When **every** backend in a health-enabled pool is down, Route still
**fails open** (re-admits backends) and Forward still runs. The client gets
a timeout **SERVFAIL**, not an immediate empty-pool SERVFAIL with no forward
attempt. The stub peer receives **no** client query for the test name
(pool members are unreachable addresses only).

Results appear on the **Conduit behavior** matrix (one stub peer).

## How it works

1. The stub peer container still starts for cell readiness, but the pool lists
   only two dead addresses (`172.30.97.98` / `.99`).
2. Health probes mark both down; the case sleeps before the client dig.
3. Conduit fails open, selects a down backend, and times out the forward
   (`max_attempts: 1`, short `forward.timeout_ms`).
4. Checks require SERVFAIL, zero peer queries for the name, one forward
   error attempt, timeout reason incremented, and **no** `no_backend` error.

## Outcomes

| Outcome | Meaning for operators |
|---|---|
| <span class="interop-outcome interop-outcome--pass">pass</span> | All-down failed open into a forward timeout SERVFAIL (not Route empty). |
| <span class="interop-outcome interop-outcome--fail">fail</span> | Unexpected rcode, peer traffic, or `no_backend` instead of timeout. |
| <span class="interop-outcome interop-outcome--skip">skip</span> | This peer/profile combination is out of scope. |
| <span class="interop-outcome interop-outcome--characterized">characterized</span> | Not used for this case today. If it appeared, it would mean a documented peer-specific quirk rather than a Conduit regression. |

**Matrix:** conduit ([Conduit behavior](/interop/conduit-behavior.md))

**Suites:** full

**Oracles:** property, peer-query-count, metrics-delta
