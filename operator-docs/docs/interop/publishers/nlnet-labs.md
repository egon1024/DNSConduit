# NLnet Labs

NLnet Labs products under test for DNSConduit correctness. No peer is preferred or recommended. See the [interop overview](/interop/index.md).

*Last tested 2026-07-18 · No failures; 2 characterized*

## Unbound

**Role:** recursive

### Profile: `forward-only`

| Test | 1.21 | 1.22 |
| --- | --- | --- |
| [`auth-dangling-cname-nxdomain`](/interop/cases/auth-dangling-cname-nxdomain.md) | <span class="interop-outcome interop-outcome--skip">skip</span> | <span class="interop-outcome interop-outcome--skip">skip</span> |
| [`auth-unknown-zone-refused`](/interop/cases/auth-unknown-zone-refused.md) | <span class="interop-outcome interop-outcome--skip">skip</span> | <span class="interop-outcome interop-outcome--skip">skip</span> |
| [`basic-a-forward`](/interop/cases/basic-a-forward.md) | <span class="interop-outcome interop-outcome--pass">pass</span> | <span class="interop-outcome interop-outcome--pass">pass</span> |
| [`dnsmasq-cname-ignored-nonlocal`](/interop/cases/dnsmasq-cname-ignored-nonlocal.md) | <span class="interop-outcome interop-outcome--skip">skip</span> | <span class="interop-outcome interop-outcome--skip">skip</span> |
| [`dnsmasq-local-cname-expand`](/interop/cases/dnsmasq-local-cname-expand.md) | <span class="interop-outcome interop-outcome--skip">skip</span> | <span class="interop-outcome interop-outcome--skip">skip</span> |
| [`edns-opt-passthrough-smoke`](/interop/cases/edns-opt-passthrough-smoke.md) | <span class="interop-outcome interop-outcome--pass">pass</span> | <span class="interop-outcome interop-outcome--pass">pass</span> |
| [`fixture-auth-a`](/interop/cases/fixture-auth-a.md) | <span class="interop-outcome interop-outcome--skip">skip</span> | <span class="interop-outcome interop-outcome--skip">skip</span> |
| [`fixture-auth-aaaa`](/interop/cases/fixture-auth-aaaa.md) | <span class="interop-outcome interop-outcome--skip">skip</span> | <span class="interop-outcome interop-outcome--skip">skip</span> |
| [`fixture-auth-api-a`](/interop/cases/fixture-auth-api-a.md) | <span class="interop-outcome interop-outcome--skip">skip</span> | <span class="interop-outcome interop-outcome--skip">skip</span> |
| [`fixture-auth-cname-a`](/interop/cases/fixture-auth-cname-a.md) | <span class="interop-outcome interop-outcome--skip">skip</span> | <span class="interop-outcome interop-outcome--skip">skip</span> |
| [`fixture-auth-nodata-mx`](/interop/cases/fixture-auth-nodata-mx.md) | <span class="interop-outcome interop-outcome--skip">skip</span> | <span class="interop-outcome interop-outcome--skip">skip</span> |
| [`fixture-auth-nxdomain`](/interop/cases/fixture-auth-nxdomain.md) | <span class="interop-outcome interop-outcome--skip">skip</span> | <span class="interop-outcome interop-outcome--skip">skip</span> |
| [`forward-parity-smoke`](/interop/cases/forward-parity-smoke.md) | <span class="interop-outcome interop-outcome--pass">pass</span> | <span class="interop-outcome interop-outcome--pass">pass</span> |
| [`passthrough-aa-auth-a`](/interop/cases/passthrough-aa-auth-a.md) | <span class="interop-outcome interop-outcome--skip">skip</span> | <span class="interop-outcome interop-outcome--skip">skip</span> |
| [`pdns-recursor-rd0-refused`](/interop/cases/pdns-recursor-rd0-refused.md) | <span class="interop-outcome interop-outcome--skip">skip</span> | <span class="interop-outcome interop-outcome--skip">skip</span> |
| [`peer-tc-passthrough`](/interop/cases/peer-tc-passthrough.md) | <span class="interop-outcome interop-outcome--skip">skip</span> | <span class="interop-outcome interop-outcome--skip">skip</span> |
| [`recursive-outside-aa-shape`](/interop/cases/recursive-outside-aa-shape.md) | <span class="interop-outcome interop-outcome--characterized">characterized</span> | <span class="interop-outcome interop-outcome--characterized">characterized</span> |
| [`rules-qname-forward`](/interop/cases/rules-qname-forward.md) | <span class="interop-outcome interop-outcome--pass">pass</span> | <span class="interop-outcome interop-outcome--pass">pass</span> |
| [`stub-aaaa-for-a-only`](/interop/cases/stub-aaaa-for-a-only.md) | <span class="interop-outcome interop-outcome--skip">skip</span> | <span class="interop-outcome interop-outcome--skip">skip</span> |
| [`unbound-local-cname-no-chase`](/interop/cases/unbound-local-cname-no-chase.md) | <span class="interop-outcome interop-outcome--pass">pass</span> | <span class="interop-outcome interop-outcome--pass">pass</span> |

Profiles with no in-scope peer-contract cases for this product: `cache-forward`, `forward-split-io` (out of scope — not failures).

| Version | Peer id | Image |
|---------|---------|-------|
| 1.21 | `nlnetlabs-unbound-1.21` | `mvance/unbound:1.21.1` |
| 1.22 | `nlnetlabs-unbound-1.22` | `mvance/unbound:1.22.0` |

