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
| Generated at | `2026-07-14T12:49:03Z` |
| Conduit version | `dev` |
| Conduit image | `conduit:local` |
| Conduit image digest (lab) | `sha256:d7b438f290547e723c76f9367bb8c2a725adc5432712f9391f571b1b10a512a6` |
| Inputs fingerprint | `sha256:4bf527f6681451be50bf8c8dd476afeaf0780faada10e488cf05f1d43d2fe994` |

The **inputs fingerprint** is a sha256 over harness inputs (`interop/catalog`, fixtures, compose, runner, results schema). CI uses it to detect when committed matrix results are stale relative to those inputs. It is not a product version.

## Outcomes

| Outcome | Meaning |
|---------|---------|
| <span class="interop-outcome interop-outcome--pass">pass</span> | Declared oracles succeeded |
| <span class="interop-outcome interop-outcome--fail">fail</span> | Unexpected mismatch or error |
| <span class="interop-outcome interop-outcome--skip">skip</span> | Case not applicable to this peer/profile |
| <span class="interop-outcome interop-outcome--characterized">characterized</span> | Expected peer-specific behavior (see case intent) |

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

