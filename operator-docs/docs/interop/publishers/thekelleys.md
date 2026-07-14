# thekelleys

thekelleys products under test for DNSConduit correctness. No peer is preferred or recommended. See the [interop overview](/interop/index.md).

*Last tested 2026-07-14 · All executed cases passed*

## dnsmasq

**Role:** stub

### Profile: `forward-only`

| Test | 2.90 |
| --- | --- |
| [`basic-a-forward`](/interop/cases/basic-a-forward.md) | <span class="interop-outcome interop-outcome--pass">pass</span> |
| [`fixture-auth-a`](/interop/cases/fixture-auth-a.md) | <span class="interop-outcome interop-outcome--skip">skip</span> |
| [`fixture-auth-aaaa`](/interop/cases/fixture-auth-aaaa.md) | <span class="interop-outcome interop-outcome--skip">skip</span> |
| [`fixture-auth-api-a`](/interop/cases/fixture-auth-api-a.md) | <span class="interop-outcome interop-outcome--skip">skip</span> |
| [`fixture-auth-nxdomain`](/interop/cases/fixture-auth-nxdomain.md) | <span class="interop-outcome interop-outcome--skip">skip</span> |
| [`forward-parity-smoke`](/interop/cases/forward-parity-smoke.md) | <span class="interop-outcome interop-outcome--pass">pass</span> |
| [`rules-qname-forward`](/interop/cases/rules-qname-forward.md) | <span class="interop-outcome interop-outcome--pass">pass</span> |

Profiles with no in-scope peer-contract cases for this product: `cache-forward`, `forward-split-io` (out of scope — not failures).

| Version | Peer id | Image |
|---------|---------|-------|
| 2.90 | `thekelleys-dnsmasq-2.90` | `strm/dnsmasq:latest` |

