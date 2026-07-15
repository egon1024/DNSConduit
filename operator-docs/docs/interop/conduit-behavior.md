# Conduit behavior

These cases exercise **Conduit** (lookup/cache path, request rules, dataplane runtime, backend health) rather than peer-product interoperability. They run against a **single stub peer** (`thekelleys-dnsmasq-2.90` (dnsmasq 2.90)) so results are not spread across every publisher column. Cache hit proofs that use `peer-query-count` rely on that stub’s dnsmasq query logs — not a general multi-peer count facility. Peer contract cases remain under [By publisher](/interop/publishers/thekelleys.md).

*Last tested 2026-07-15 · All executed cases passed*

## Results

| Test | `cache-forward` | `forward-only` | `forward-split-io` |
| --- | --- | --- | --- |
| [`cache-forward-a`](/interop/cases/cache-forward-a.md) | <span class="interop-outcome interop-outcome--pass">pass</span> | <span class="interop-outcome interop-outcome--skip">skip</span> | <span class="interop-outcome interop-outcome--skip">skip</span> |
| [`cache-miss-then-hit-a`](/interop/cases/cache-miss-then-hit-a.md) | <span class="interop-outcome interop-outcome--pass">pass</span> | <span class="interop-outcome interop-outcome--skip">skip</span> | <span class="interop-outcome interop-outcome--skip">skip</span> |
| [`cache-nxdomain-miss-then-hit`](/interop/cases/cache-nxdomain-miss-then-hit.md) | <span class="interop-outcome interop-outcome--pass">pass</span> | <span class="interop-outcome interop-outcome--skip">skip</span> | <span class="interop-outcome interop-outcome--skip">skip</span> |
| [`cache-rotate-rrset`](/interop/cases/cache-rotate-rrset.md) | <span class="interop-outcome interop-outcome--pass">pass</span> | <span class="interop-outcome interop-outcome--skip">skip</span> | <span class="interop-outcome interop-outcome--skip">skip</span> |
| [`dataplane-split-io-forward`](/interop/cases/dataplane-split-io-forward.md) | <span class="interop-outcome interop-outcome--skip">skip</span> | <span class="interop-outcome interop-outcome--skip">skip</span> | <span class="interop-outcome interop-outcome--pass">pass</span> |
| [`health-fail-open-all-down`](/interop/cases/health-fail-open-all-down.md) | <span class="interop-outcome interop-outcome--skip">skip</span> | <span class="interop-outcome interop-outcome--pass">pass</span> | <span class="interop-outcome interop-outcome--skip">skip</span> |
| [`health-live-dead-prefer-live`](/interop/cases/health-live-dead-prefer-live.md) | <span class="interop-outcome interop-outcome--skip">skip</span> | <span class="interop-outcome interop-outcome--pass">pass</span> | <span class="interop-outcome interop-outcome--skip">skip</span> |
| [`rhai-soft-drop`](/interop/cases/rhai-soft-drop.md) | <span class="interop-outcome interop-outcome--skip">skip</span> | <span class="interop-outcome interop-outcome--pass">pass</span> | <span class="interop-outcome interop-outcome--skip">skip</span> |
| [`rules-clear-drop`](/interop/cases/rules-clear-drop.md) | <span class="interop-outcome interop-outcome--skip">skip</span> | <span class="interop-outcome interop-outcome--pass">pass</span> | <span class="interop-outcome interop-outcome--skip">skip</span> |
| [`rules-drop-now`](/interop/cases/rules-drop-now.md) | <span class="interop-outcome interop-outcome--skip">skip</span> | <span class="interop-outcome interop-outcome--pass">pass</span> | <span class="interop-outcome interop-outcome--skip">skip</span> |
| [`rules-soft-drop`](/interop/cases/rules-soft-drop.md) | <span class="interop-outcome interop-outcome--skip">skip</span> | <span class="interop-outcome interop-outcome--pass">pass</span> | <span class="interop-outcome interop-outcome--skip">skip</span> |

Stub peer id: `thekelleys-dnsmasq-2.90`. Cases declare `matrix: conduit` and pin this peer in `applicability.peers`.

