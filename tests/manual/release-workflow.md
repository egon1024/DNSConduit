# Manual testing: release workflow

Use when validating release automation or recovering from a failed **Release** workflow run.

## Normal path (merge to main)

1. Merge a pull request to `main` with a valid **`#major`**, **`#minor`**, or **`#patch`** line in the description.
2. **Release** workflow runs (not on `chore: release …` commits).
3. Expected order:
   - Compute `NEXT_VERSION`
   - Bump `Cargo.toml`
   - Finalize `operator-docs/docs/release-notes/unreleased.md` → `{NEXT_VERSION}.md`, reset unreleased
   - `make docs-build` (strict MkDocs)
   - Commit `chore: release {NEXT_VERSION}` to `main` (Cargo + release notes)
   - Verify notes and Cargo version on `origin/main`
   - `gh release create` (tag + GitHub Release with generated PR notes)
   - Dispatch **Docs deploy** and **Build release artifacts**

A version appears publicly only after **`gh release create`** succeeds. Docs deploy uses the new tag; GitHub Pages does not list the version until **Docs deploy** completes.

## Local dry-run (finalize script)

```bash
# From a copy of unreleased.md content:
python3 operator-docs/scripts/finalize_release_notes.py \
  --version 0.99.0 \
  --repo "https://github.com/egon1024/DNSConduit"

# Revert test output before committing:
git checkout -- operator-docs/docs/release-notes/
```

Run unit tests:

```bash
python3 -m unittest discover -s operator-docs/scripts -p 'test_*.py'
```

## Failure recovery

| Failure point | On `main` | Tag / GitHub Release | Docs site | Action |
|---------------|-----------|----------------------|-----------|--------|
| Finalize or `docs-build` | Unchanged | None | Unchanged | Fix `unreleased.md` or docs; merge fix; re-merge or re-run release trigger |
| Commit / PR merge | Unchanged | None | Unchanged | Fix `RELEASE_PUSH_TOKEN` or branch protection; re-run **Release** |
| Verify on main | May have `chore: release` commit | None | Unchanged | Inspect `main`; fix missing notes or Cargo mismatch manually, then retry |
| `gh release create` | Cargo bump + notes committed | **Missing** | Unchanged | **Workflow dispatch** → **Release** → set **`release_version`** to the prepared version (leave bump empty) |
| Docs deploy | OK | OK | Old version | **Workflow dispatch** → **Docs deploy** → `version={tag}` |
| Release artifacts | OK | OK | OK | Re-dispatch **Build release artifacts** (see [release-artifacts.md](release-artifacts.md)) |

### Retry GitHub Release only

Use when `main` already has `Cargo.toml` and `operator-docs/docs/release-notes/{version}.md` but the release step failed:

```bash
gh workflow run release.yml -f release_version=0.14.0
```

The workflow skips finalize when the version page already exists, skips commit when there is nothing to commit, then creates the release and dispatches downstream jobs.

### Retry docs deploy only

```bash
gh workflow run docs-deploy.yml --ref 0.14.0 -f version=0.14.0
```

### Accidental partial state on main (no tag)

If `main` has a `chore: release X.Y.Z` commit but no tag:

1. Confirm `operator-docs/docs/release-notes/X.Y.Z.md` exists on `main`.
2. Run **Release** with `release_version=X.Y.Z` as above.

Do **not** delete the release notes file or revert Cargo unless you are intentionally aborting the release.

### Abort before tag

If the wrong version was prepared on `main` but **no** GitHub Release exists yet:

1. Revert the `chore: release …` commit on `main` (maintainer action).
2. Restore bullets to `unreleased.md` if needed.
3. Ship a corrective merge and let **Release** run again.

## Conflicting edits to `unreleased.md`

If two pull requests conflict on `unreleased.md`, resolve the merge conflict manually (combine bullets under the right `##` headings). If this becomes frequent, migrate to `operator-docs/changelog.d/*.md` fragments (one file per PR) and extend `finalize_release_notes.py` to collect them.
