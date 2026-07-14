# ISC

ISC products under test for DNSConduit correctness. No peer is preferred or recommended. See the [interop overview](/interop/index.md) for provenance shared across publishers.

## Last tested

| Field | Value |
|-------|-------|
| Generated at | `2026-07-14T12:49:03Z` |
| Conduit version | `dev` |
| Conduit image | `conduit:local` |
| Conduit image digest (lab) | `sha256:d7b438f290547e723c76f9367bb8c2a725adc5432712f9391f571b1b10a512a6` |
| Inputs fingerprint | `sha256:4bf527f6681451be50bf8c8dd476afeaf0780faada10e488cf05f1d43d2fe994` |

The **inputs fingerprint** is a sha256 over harness inputs (`interop/catalog`, fixtures, compose, runner, results schema). CI uses it to detect when committed matrix results are stale relative to those inputs. It is not a product version.

## BIND

**Role:** auth

### Profile: `forward-only`

| Test | 9.18 | 9.20 |
| --- | --- | --- |
| [`basic-a-forward`](/interop/cases/basic-a-forward.md) | <span class="interop-outcome interop-outcome--pass">pass</span> | <span class="interop-outcome interop-outcome--pass">pass</span> |
| [`fixture-auth-a`](/interop/cases/fixture-auth-a.md) | <span class="interop-outcome interop-outcome--pass">pass</span> | <span class="interop-outcome interop-outcome--pass">pass</span> |
| [`lab-forward-parity-smoke`](/interop/cases/lab-forward-parity-smoke.md) | <span class="interop-outcome interop-outcome--pass">pass</span> | <span class="interop-outcome interop-outcome--pass">pass</span> |

| Version | Peer id | Image |
|---------|---------|-------|
| 9.18 | `isc-bind-9.18` | `internetsystemsconsortium/bind9:9.18` |
| 9.20 | `isc-bind-9.20` | `internetsystemsconsortium/bind9:9.20` |

