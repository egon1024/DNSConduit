# Interop

Published correctness results for DNSConduit against peer DNS software under test. **Peer contract** cases are split **by publisher** (alphabetical). **Conduit behavior** cases (cache path, rules, dataplane runtime) use a single stub peer — see [Conduit behavior](/interop/conduit-behavior.md). No peer is preferred or recommended.

By default, Conduit’s forward path **passes peer response shapes through** (rcode, answer section, and flags such as AA/TC) so operators see the same backend quirks they would when querying the peer directly. Cases that document those quirks use parity against a direct dig; `characterized` cells record expected peer-specific shapes. Configuration that rewrites or sanitizes peer answers is covered separately when those knobs are under test.

## Matrices

- [Conduit behavior](/interop/conduit-behavior.md) — Conduit-focused cases (stub peer)

## Publishers

- [CZ.NIC](/interop/publishers/cz-nic.md) — Knot DNS
- [ISC](/interop/publishers/isc.md) — BIND, BIND Resolver
- [NLnet Labs](/interop/publishers/nlnet-labs.md) — Unbound
- [PowerDNS](/interop/publishers/powerdns.md) — Authoritative Server, Recursor
- [thekelleys](/interop/publishers/thekelleys.md) — dnsmasq

*Last tested 2026-07-15 · No failures; 8 characterized*

## Outcomes

| Outcome | Meaning |
|---------|---------|
| <span class="interop-outcome interop-outcome--pass">pass</span> | Case checks met for this peer/version — the declared contract holds |
| <span class="interop-outcome interop-outcome--fail">fail</span> | Unexpected mismatch or error — investigate Conduit forwarding or the peer path for this cell |
| <span class="interop-outcome interop-outcome--skip">skip</span> | Out of scope for this peer role or profile (not a failure) |
| <span class="interop-outcome interop-outcome--characterized">characterized</span> | Documented peer-specific behavior (see the case page), not treated as a Conduit regression |

Each [case page](/interop/cases/basic-a-forward.md) explains purpose, how the test runs, and what these outcomes mean for that case.

## Running these tests locally

The matrices on this site are from a committed lab run. You can reproduce or explore the same harness on a machine with **Docker**, **Docker Compose**, and **Python 3** (with PyYAML). GitHub Actions does **not** execute the Docker suite; CI only checks that committed results stay fresh when harness inputs change.

From a checkout of the DNSConduit repository:

1. **Build a Conduit image** used as the system under test:

    ```zsh
    make interop-image
    ```

    This builds `conduit:local` via the repo `Dockerfile`. Override with `CONDUIT_IMAGE=…` if you already have an image tag.

2. **Run the smoke suite** (all peers the smoke cases apply to). Peer images are pulled as needed; the first run can take a while:

    ```zsh
    make interop-smoke
    ```

3. **Optional — authoritative fixture case** (auth peers only):

    ```zsh
    make interop-auth
    ```

Those targets **print** pass/fail/skip lines; they do **not** rewrite `interop/results/latest.json` or regenerate this site. Named [cases](/interop/cases/basic-a-forward.md) document purpose, how each test works, and outcome implications.

Useful extras:

| Command | What it does |
|---------|--------------|
| `make interop-unit` | Fast harness unit tests (no Docker cells) |
| `make interop-docs` | Rebuild these matrix pages from the committed `latest.json` |
| `make interop-refresh` | Rebuild image, re-run smoke + auth, **write** results and regenerate docs (maintainers) |

Filters (peer, case, profile) and pack layout: see `interop/README.md` in the repository. Override the image for any run target with `make interop-smoke CONDUIT_IMAGE=registry.example/conduit:1.2.3`.

## Summary

| Outcome | Test | Publisher | Product | Version | Profile |
|---------|------|-----------|---------|---------|---------|
| <span class="interop-outcome interop-outcome--characterized">characterized</span> | [`dnsmasq-cname-ignored-nonlocal`](/interop/cases/dnsmasq-cname-ignored-nonlocal.md) | [thekelleys](/interop/publishers/thekelleys.md) | dnsmasq | 2.90 | `forward-only` |
| <span class="interop-outcome interop-outcome--characterized">characterized</span> | [`pdns-recursor-rd0-refused`](/interop/cases/pdns-recursor-rd0-refused.md) | [PowerDNS](/interop/publishers/powerdns.md) | Recursor | 5.3 | `forward-only` |
| <span class="interop-outcome interop-outcome--characterized">characterized</span> | [`pdns-recursor-rd0-refused`](/interop/cases/pdns-recursor-rd0-refused.md) | [PowerDNS](/interop/publishers/powerdns.md) | Recursor | 5.4 | `forward-only` |
| <span class="interop-outcome interop-outcome--characterized">characterized</span> | [`recursive-outside-aa-shape`](/interop/cases/recursive-outside-aa-shape.md) | [NLnet Labs](/interop/publishers/nlnet-labs.md) | Unbound | 1.21 | `forward-only` |
| <span class="interop-outcome interop-outcome--characterized">characterized</span> | [`recursive-outside-aa-shape`](/interop/cases/recursive-outside-aa-shape.md) | [NLnet Labs](/interop/publishers/nlnet-labs.md) | Unbound | 1.22 | `forward-only` |
| <span class="interop-outcome interop-outcome--characterized">characterized</span> | [`recursive-outside-aa-shape`](/interop/cases/recursive-outside-aa-shape.md) | [PowerDNS](/interop/publishers/powerdns.md) | Recursor | 5.3 | `forward-only` |
| <span class="interop-outcome interop-outcome--characterized">characterized</span> | [`recursive-outside-aa-shape`](/interop/cases/recursive-outside-aa-shape.md) | [PowerDNS](/interop/publishers/powerdns.md) | Recursor | 5.4 | `forward-only` |
| <span class="interop-outcome interop-outcome--characterized">characterized</span> | [`stub-aaaa-for-a-only`](/interop/cases/stub-aaaa-for-a-only.md) | [thekelleys](/interop/publishers/thekelleys.md) | dnsmasq | 2.90 | `forward-only` |

## Peer catalog

Peers are software under test. Ordering is publisher (A–Z), product (A–Z), version ascending.

| Publisher | Product | Version | Role | Id |
|-----------|---------|---------|------|----|
| [CZ.NIC](/interop/publishers/cz-nic.md) | Knot DNS | 3.4 | auth | `cznic-knot-3.4` |
| [CZ.NIC](/interop/publishers/cz-nic.md) | Knot DNS | 3.5 | auth | `cznic-knot-3.5` |
| [ISC](/interop/publishers/isc.md) | BIND | 9.18 | auth | `isc-bind-9.18` |
| [ISC](/interop/publishers/isc.md) | BIND | 9.20 | auth | `isc-bind-9.20` |
| [ISC](/interop/publishers/isc.md) | BIND Resolver | 9.18 | recursive | `isc-bind-resolver-9.18` |
| [ISC](/interop/publishers/isc.md) | BIND Resolver | 9.20 | recursive | `isc-bind-resolver-9.20` |
| [NLnet Labs](/interop/publishers/nlnet-labs.md) | Unbound | 1.21 | recursive | `nlnetlabs-unbound-1.21` |
| [NLnet Labs](/interop/publishers/nlnet-labs.md) | Unbound | 1.22 | recursive | `nlnetlabs-unbound-1.22` |
| [PowerDNS](/interop/publishers/powerdns.md) | Authoritative Server | 5.0 | auth | `powerdns-auth-5.0` |
| [PowerDNS](/interop/publishers/powerdns.md) | Authoritative Server | 5.1 | auth | `powerdns-auth-5.1` |
| [PowerDNS](/interop/publishers/powerdns.md) | Recursor | 5.3 | recursive | `powerdns-recursor-5.3` |
| [PowerDNS](/interop/publishers/powerdns.md) | Recursor | 5.4 | recursive | `powerdns-recursor-5.4` |
| [thekelleys](/interop/publishers/thekelleys.md) | dnsmasq | 2.90 | stub | `thekelleys-dnsmasq-2.90` |

