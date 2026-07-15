# DNSConduit interop correctness harness

Local/lab Docker harness: DNSConduit (system under test) plus **one peer backend at a time**.
Results are committed and published as the operator-docs interop matrix.

**Workshop vs contract:** Manual labs prove features while building; this tree is the
committed contract. See `docs/superpowers/process/e2e-interop-testing.md`.

GitHub Actions does **not** run the Docker suite (the PR check only verifies results
freshness). Use the Makefile targets below for local execution.

## Quick start

```zsh
cd ~/git_repos/DNSConduit

make interop-image          # build conduit:local
make interop-smoke          # smoke suite (prints outcomes; does not rewrite results)
make interop-auth           # fixture-auth-a on auth peers
make interop-docs           # regenerate operator-docs from interop/results/latest.json
make interop-fingerprint    # inputs fingerprint (PR freshness)
make interop-unit           # harness unit tests (no Docker cells)

# Maintainers refreshing the published matrix:
make interop-refresh        # rebuild image, smoke + auth, write results + docs
```

Override the SUT image: `make interop-smoke CONDUIT_IMAGE=registry.example/conduit:1.2.3`.

Equivalent Python entry points remain available (`python3 -m interop.runner …`); prefer
`make interop-*` for stable names.

Published docs: **Interop** overview (includes local run steps) plus **By publisher**
pages (alphabetical) and **Cases**.


## Filters

```zsh
python3 -m interop.runner run \
  --case basic-a-forward \
  --peer thekelleys-dnsmasq-2.90 \
  --profile forward-only \
  --conduit-image conduit:local
```

## Layout

| Path | Purpose |
|------|---------|
| `catalog/peers.yaml` | Peer publisher / product / version / image / role / family |
| `catalog/profiles/` | Conduit YAML profiles |
| `catalog/cases/` | Named cases (metadata + intent + `peer_setup` / `conduit_delta`) |
| `fixtures/` | Harness-owned zones and expected answers |
| `runner/` | Python CLI and libraries |
| `compose/` | Compose base template (per-cell layer) |
| `peers/<family>/` | Peer family config packs (see below) |
| `results/latest.json` | Committed matrix results + provenance |

## Peer family packs

Each peer in `catalog/peers.yaml` declares a `family` (e.g. `dnsmasq`, `bind`,
`bind-recursive`). The runner resolves `interop/peers/<family>/` and layers its
`compose.override.yml` on top of `compose/cell.compose.yml` for that cell.

A case's `peer_setup` (product-neutral `fixtures` + `local_rr`) is rendered
into a per-run tempdir by the family pack:

- `templates/` — files with `$VAR`-style substitutions (`LOCAL_RR_BIND_LINES`,
  `FIXTURE_ZONE_IDS`, `CONFIG_DIR`, plus family-specific extras)
- `prepare.py` (optional) — a `prepare(out_dir, ir, peer)` hook for daemons that
  need generated config/zones beyond simple templating (e.g. dnsmasq's `run.sh`)

A case's `conduit_delta` replaces top-level keys in the selected Conduit
profile before that cell starts (e.g. swapping the `pools` backend address).

**`peer-query-count` oracle:** stub-peer **cache-hit proof** only — implemented
for the **dnsmasq** family via query logs. Conduit-behavior cache cases pin
`thekelleys-dnsmasq-2.90`; other families raise if a case requests this oracle.

Recursive/stub peers must answer from local/static data only — no public
internet dependency in committed cases. See
`docs/superpowers/process/e2e-interop-testing.md` (Cursor-side) for the
per-family expansion checklist.

## Neutrality

Peers are **software under test**. Matrix columns sort publisher A–Z, then product,
then version. Do not imply preference for any peer.

## Refreshing peer image pins

1. Pull the desired tag: `docker pull <image>:<tag>`
2. Record digest: `docker image inspect --format '{{index .RepoDigests 0}}' <image>:<tag>`
3. Update `catalog/peers.yaml` `image:` to `repo@sha256:…` (or keep tag + note digest in comments).
4. Re-run with `make interop-refresh` (or filtered `python3 -m interop.runner run …`) and
   commit `results/latest.json` plus regenerated matrix docs.
5. Prefer two minors per family (current + previous) on the published matrix; keep extras offline.

Known image naming quirks (verify on Docker Hub when refreshing):

- Knot DNS: `cznic/knot:<major.minor>` (not `cznic/knot-dns`)
- PowerDNS Authoritative: `powerdns/pdns-auth-50:5.0.x`, `powerdns/pdns-auth-51:5.1.x`
- PowerDNS Recursor: `powerdns/pdns-recursor-53:5.3.x`, `powerdns/pdns-recursor-54:5.4.x` (per-minor repos)
- BIND (auth and resolver peers): `internetsystemsconsortium/bind9:<major.minor>`
- Unbound: third-party images such as `mvance/unbound:<x.y.z>` (confirm tag exists before pin)

## Workshop graduation

When a manual lab scenario is stable, add a case under `catalog/cases/` and keep the
lab assets. See `docs/superpowers/process/e2e-interop-testing.md`.
