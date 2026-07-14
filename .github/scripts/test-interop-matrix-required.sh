#!/usr/bin/env bash
# Dry-run fixtures for interop-matrix-required.sh
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$ROOT"
SCRIPT="$ROOT/.github/scripts/interop-matrix-required.sh"

fail() { echo "FAIL: $*" >&2; exit 1; }
pass() { echo "PASS: $*"; }

# Unrelated change → pass without results check of fingerprint mismatch
# We simulate by setting BASE/HEAD to same and empty relevant — use env override via fake.
# Instead: call with #no-interop-matrix
export BASE_SHA="$(git rev-parse HEAD)"
export HEAD_SHA="$(git rev-parse HEAD)"
export PR_BODY="#no-interop-matrix reason: test"
bash "$SCRIPT" || fail "override should pass"
pass "override tag"

export PR_BODY=""
# Same tree: no changed files between BASE and HEAD → should pass as unrelated
bash "$SCRIPT" || fail "identical SHAs should pass (no changed paths)"
pass "no changed paths"

# Fingerprint self-consistency
fp="$(python3 -m interop.runner fingerprint)"
[[ "$fp" == sha256:* ]] || fail "fingerprint format"
pass "fingerprint format $fp"

echo "All interop gate dry-run checks passed."
