#!/usr/bin/env bash
# Fail if interop-relevant paths changed without matching results fingerprint
# or #no-interop-matrix in the PR body.
set -euo pipefail

PR_BODY="${PR_BODY:-}"
BASE_SHA="${BASE_SHA:?BASE_SHA is required}"
HEAD_SHA="${HEAD_SHA:?HEAD_SHA is required}"

if [[ "$PR_BODY" == *"#no-interop-matrix"* ]]; then
  echo "PR body contains #no-interop-matrix — interop matrix refresh not required."
  exit 0
fi

mapfile -t CHANGED < <(git diff --name-only "${BASE_SHA}" "${HEAD_SHA}")

relevant=false
for path in "${CHANGED[@]}"; do
  case "$path" in
    interop/* | \
    crates/conduit-dataplane/* | \
    crates/conduit-core/* | \
    crates/conduit-config/* | \
    crates/conduit/* | \
    proto/conduit/v1/* | \
    Dockerfile)
      relevant=true
      break
      ;;
  esac
done

if [[ "$relevant" != true ]]; then
  echo "No interop-relevant paths changed."
  exit 0
fi

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$ROOT"

if [[ ! -f interop/results/latest.json ]]; then
  echo "::error::interop/results/latest.json is missing; run the interop harness and commit results."
  exit 1
fi

expected="$(python3 -m interop.runner fingerprint)"
actual="$(python3 -c 'import json; print(json.load(open("interop/results/latest.json"))["inputs_fingerprint"])')"

if [[ "$expected" != "$actual" ]]; then
  echo "::error::Interop matrix results are stale."
  echo "Computed fingerprint: ${expected}"
  echo "Results fingerprint:  ${actual}"
  echo "Re-run: python3 -m interop.runner run --suite smoke --write-results --generate-matrix"
  echo "Or add #no-interop-matrix to the PR description with a reason."
  exit 1
fi

echo "Interop results fingerprint matches tree (${expected})."
exit 0
