# Interop

Published correctness results for DNSConduit against peer DNS software under test. Results are split **by publisher** (alphabetical) so version and product matrices stay readable. No peer is preferred or recommended.

## Publishers

- [CZ.NIC](/interop/publishers/cz-nic.md) — Knot DNS
- [ISC](/interop/publishers/isc.md) — BIND
- [NLnet Labs](/interop/publishers/nlnet-labs.md) — Unbound
- [PowerDNS](/interop/publishers/powerdns.md) — Authoritative Server, Recursor
- [thekelleys](/interop/publishers/thekelleys.md) — dnsmasq

## Last tested

| Field | Value |
|-------|-------|
| Generated at | `2026-07-14T14:31:10Z` |
| Conduit version | `dev` |
| Conduit image | `conduit:local` |
| Conduit image digest (lab) | `sha256:35197eb9c8138614c75ea7af788d6d6d543bb3ec0d74b393f4aaf9cf71dee2b3` |
| Inputs fingerprint | `sha256:df2ce7febd8b6b71aa2a913d43e43fb20db578d93fd9c7bdfc7d8331b2044efc` |

The **inputs fingerprint** is a sha256 over harness inputs (`interop/catalog`, fixtures, compose, runner, results schema). CI uses it to detect when committed matrix results are stale relative to those inputs. It is not a product version.

## Outcomes

| Outcome | Meaning |
|---------|---------|
| <span class="interop-outcome interop-outcome--pass">pass</span> | Case checks met for this peer/version — the declared contract holds |
| <span class="interop-outcome interop-outcome--fail">fail</span> | Unexpected mismatch or error — investigate Conduit forwarding or the peer path for this cell |
| <span class="interop-outcome interop-outcome--skip">skip</span> | Out of scope for this peer role or profile (not a failure) |
| <span class="interop-outcome interop-outcome--characterized">characterized</span> | Documented peer-specific behavior (see the case page), not treated as a Conduit regression |

Each [case page](/interop/cases/basic-a-forward.md) explains purpose, how the test runs, and what these outcomes mean for that case.

## Summary

No `fail` or `characterized` cells in the committed results. Open a publisher page for the full matrix (including `pass` and `skip`).

## Peer catalog

Peers are software under test. Ordering is publisher (A–Z), product (A–Z), version ascending.

| Publisher | Product | Version | Role | Id |
|-----------|---------|---------|------|----|
| [CZ.NIC](/interop/publishers/cz-nic.md) | Knot DNS | 3.4 | auth | `cznic-knot-3.4` |
| [CZ.NIC](/interop/publishers/cz-nic.md) | Knot DNS | 3.5 | auth | `cznic-knot-3.5` |
| [ISC](/interop/publishers/isc.md) | BIND | 9.18 | auth | `isc-bind-9.18` |
| [ISC](/interop/publishers/isc.md) | BIND | 9.20 | auth | `isc-bind-9.20` |
| [NLnet Labs](/interop/publishers/nlnet-labs.md) | Unbound | 1.21 | recursive | `nlnetlabs-unbound-1.21` |
| [NLnet Labs](/interop/publishers/nlnet-labs.md) | Unbound | 1.22 | recursive | `nlnetlabs-unbound-1.22` |
| [PowerDNS](/interop/publishers/powerdns.md) | Authoritative Server | 5.0 | auth | `powerdns-auth-5.0` |
| [PowerDNS](/interop/publishers/powerdns.md) | Authoritative Server | 5.1 | auth | `powerdns-auth-5.1` |
| [PowerDNS](/interop/publishers/powerdns.md) | Recursor | 5.3 | recursive | `powerdns-recursor-5.3` |
| [PowerDNS](/interop/publishers/powerdns.md) | Recursor | 5.4 | recursive | `powerdns-recursor-5.4` |
| [thekelleys](/interop/publishers/thekelleys.md) | dnsmasq | 2.90 | stub | `thekelleys-dnsmasq-2.90` |

