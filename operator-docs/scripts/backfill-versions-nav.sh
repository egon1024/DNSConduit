#!/usr/bin/env bash
#
# One-time maintenance: repoint the "Versions" navigation entry of already-published
# documentation versions at the single global Versions page (site root /versions/).
#
# Background: each published version directory was built from that tag's source, which
# linked a per-version "versions.md" page. Plain `mike deploy <oldtag>` would rebuild
# from that old source and keep the stale per-version list, so this script overlays the
# current nav change (absolute link to the global page) onto each tag before rebuilding.
#
# It rebuilds only the version directory for each tag and DOES NOT move aliases or change
# the site default — alias/`set-default` state is left exactly as mike currently has it.
#
# Usage:
#   operator-docs/scripts/backfill-versions-nav.sh [--push] [TAG ...]
#
#   --push        Push each rebuilt version to the gh-pages branch (omit for a dry run
#                 that builds locally without publishing).
#   TAG ...       Tags to backfill. Defaults to: 0.12.0 0.13.0 0.14.0 0.15.0
#
# Prerequisites:
#   - Run from the repository root with mkdocs-material + mike installed
#     (pip install -r requirements.txt).
#   - `git fetch --tags` so the requested tags exist locally.
#   - Write access to the gh-pages branch when using --push.
#
# Recommended: spot-check a single old tag first, e.g.
#   operator-docs/scripts/backfill-versions-nav.sh 0.14.0          # local build only
#   operator-docs/scripts/backfill-versions-nav.sh --push 0.14.0   # publish one tag
# then run the full set.

set -euo pipefail

GLOBAL_VERSIONS_URL="https://egon1024.github.io/DNSConduit/versions/"

PUSH=0
TAGS=()
for arg in "$@"; do
  case "${arg}" in
    --push) PUSH=1 ;;
    -*) echo "Unknown option: ${arg}" >&2; exit 2 ;;
    *) TAGS+=("${arg}") ;;
  esac
done

if [[ ${#TAGS[@]} -eq 0 ]]; then
  TAGS=(0.12.0 0.13.0 0.14.0 0.15.0)
fi

if [[ ! -f operator-docs/mkdocs.yml ]]; then
  echo "::error::run this script from the repository root (operator-docs/mkdocs.yml not found)" >&2
  exit 1
fi

git config user.name >/dev/null 2>&1 || {
  echo "::error::git user.name/email must be configured for mike commits" >&2
  exit 1
}

echo "Backfilling Versions nav for: ${TAGS[*]}"
echo "Push to gh-pages: $([[ ${PUSH} -eq 1 ]] && echo yes || echo 'no (dry run)')"

for tag in "${TAGS[@]}"; do
  echo "==> ${tag}"
  if ! git rev-parse -q --verify "refs/tags/${tag}" >/dev/null; then
    echo "::error::tag ${tag} not found locally (run: git fetch --tags)" >&2
    exit 1
  fi

  work="$(mktemp -d)"
  cleanup() { git worktree remove "${work}" --force >/dev/null 2>&1 || true; }
  trap cleanup EXIT

  git worktree add --detach "${work}" "${tag}" >/dev/null

  mkdocs_yml="${work}/operator-docs/mkdocs.yml"
  if [[ ! -f "${mkdocs_yml}" ]]; then
    echo "::error::${tag} has no operator-docs/mkdocs.yml; cannot backfill" >&2
    cleanup; trap - EXIT; exit 1
  fi

  # Overlay the Versions nav entry -> absolute link to the global page.
  if grep -qE '^[[:space:]]*-[[:space:]]+Versions:' "${mkdocs_yml}"; then
    sed -i -E "s#^([[:space:]]*-[[:space:]]+Versions:).*#\1 ${GLOBAL_VERSIONS_URL}#" "${mkdocs_yml}"
  else
    echo "::warning::${tag} mkdocs.yml has no 'Versions:' nav entry; leaving nav unchanged"
  fi

  # Repoint in-page links to the old per-version page at the global page, so the
  # strict build does not abort on a dangling 'versions.md' link once it is removed.
  if grep -rlZ 'versions\.md' "${work}/operator-docs/docs" >/dev/null 2>&1; then
    grep -rlZ 'versions\.md' "${work}/operator-docs/docs" \
      | xargs -0 sed -i -E "s@\]\((\.{0,2}/)?versions\.md(#[^)]*)?\)@](${GLOBAL_VERSIONS_URL})@g"
  fi

  # Drop the per-version page so it cannot orphan the strict build.
  rm -f "${work}/operator-docs/docs/versions.md"
  # Header version label for this rebuild.
  echo "${tag}" > "${work}/operator-docs/.doc-version"

  (
    cd "${work}/operator-docs"
    export DOCS_PRODUCT_VERSION="${tag}"
    if [[ ${PUSH} -eq 1 ]]; then
      git fetch origin gh-pages:gh-pages 2>/dev/null || true
      # No alias flags and no set-default: this only refreshes the version directory.
      mike deploy --push "${tag}"
    else
      mike deploy "${tag}"
      echo "    (dry run) rebuilt ${tag} locally; re-run with --push to publish"
    fi
  )

  cleanup
  trap - EXIT
done

echo "Backfill complete."
