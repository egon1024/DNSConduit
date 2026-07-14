# thekelleys

thekelleys products under test for DNSConduit correctness. No peer is preferred or recommended. See the [interop overview](/interop/index.md) for provenance shared across publishers.

## Last tested

| Field | Value |
|-------|-------|
| Generated at | `2026-07-14T14:31:10Z` |
| Conduit version | `dev` |
| Conduit image | `conduit:local` |
| Conduit image digest (lab) | `sha256:35197eb9c8138614c75ea7af788d6d6d543bb3ec0d74b393f4aaf9cf71dee2b3` |
| Inputs fingerprint | `sha256:df2ce7febd8b6b71aa2a913d43e43fb20db578d93fd9c7bdfc7d8331b2044efc` |

The **inputs fingerprint** is a sha256 over harness inputs (`interop/catalog`, fixtures, compose, runner, results schema). CI uses it to detect when committed matrix results are stale relative to those inputs. It is not a product version.

## dnsmasq

**Role:** stub

### Profile: `forward-only`

| Test | 2.90 |
| --- | --- |
| [`basic-a-forward`](/interop/cases/basic-a-forward.md) | <span class="interop-outcome interop-outcome--pass">pass</span> |
| [`fixture-auth-a`](/interop/cases/fixture-auth-a.md) | <span class="interop-outcome interop-outcome--skip">skip</span> |
| [`lab-forward-parity-smoke`](/interop/cases/lab-forward-parity-smoke.md) | <span class="interop-outcome interop-outcome--pass">pass</span> |

| Version | Peer id | Image |
|---------|---------|-------|
| 2.90 | `thekelleys-dnsmasq-2.90` | `strm/dnsmasq:latest` |

