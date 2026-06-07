# Manual testing: release artifact workflow

Use after merging release-artifacts changes, when a semver GitHub release already exists.

## Prerequisites

- `gh` authenticated with access to the repository
- An existing release tag (e.g. `0.13.0`) created by `release.yml`

## Dispatch artifact build

```bash
gh workflow run build-release-artifacts.yml \
  --ref 0.13.0 \
  -f version=0.13.0
```

Watch the run:

```bash
gh run list --workflow=build-release-artifacts.yml
gh run watch
```

## Expect on the release page

- `conduit-<version>-amd64.tar.gz`
- `conduit-<version>-amd64-debug.tar.gz`
- `conduit_<version>_amd64.deb`
- `conduit-dbg_<version>_amd64.deb`
- `SHA256SUMS`
- `conduit-<version>.spdx.json` (SBOM)

## Rebuild

Delete existing assets from the GitHub release UI (or `gh release delete-asset`), then re-dispatch. The workflow **fails** if any expected asset name already exists.

## Local dry-run (no upload)

```bash
VERSION=0.13.0
cargo build --release -p conduit -p conduitctl -p conduit-dnstap-tracer
VERSION="$VERSION" bash .github/scripts/build-release-artifacts.sh
ls release-artifacts/
```

Requires `nfpm` on PATH for `.deb` generation.
