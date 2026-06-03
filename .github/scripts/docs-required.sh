#!/usr/bin/env bash
# Fail if operator-visible paths changed without operator-docs/, README.md, or #no-docs.
set -euo pipefail

PR_BODY="${PR_BODY:-}"
BASE_SHA="${BASE_SHA:?BASE_SHA is required}"
HEAD_SHA="${HEAD_SHA:?HEAD_SHA is required}"

if [[ "$PR_BODY" == *"#no-docs"* ]]; then
  echo "PR body contains #no-docs — documentation update not required."
  exit 0
fi

mapfile -t CHANGED < <(git diff --name-only "${BASE_SHA}" "${HEAD_SHA}")

docs_updated=false
for path in "${CHANGED[@]}"; do
  if [[ "$path" == operator-docs/* || "$path" == README.md ]]; then
    docs_updated=true
    break
  fi
done

tier_a=false
tier_b=false
contract_signal=false

for path in "${CHANGED[@]}"; do
  case "$path" in
    proto/conduit/v1/* | crates/conduit-config/* | crates/conduitctl/* | crates/conduit-api/* \
      | crates/conduit-metrics/* | crates/conduit-events/* | crates/conduit-script/* \
      | tests/fixtures/config/*)
      tier_a=true
      ;;
  esac
  case "$path" in
    crates/conduit-dataplane/*)
      tier_b=true
      ;;
  esac
  case "$path" in
    proto/conduit/v1/* | crates/conduit-config/*)
      contract_signal=true
      ;;
  esac
done

docs_required=false
if [[ "$tier_a" == true ]]; then
  docs_required=true
fi
if [[ "$tier_b" == true && "$contract_signal" == true ]]; then
  docs_required=true
fi

if [[ "$docs_required" != true ]]; then
  echo "No operator-surface paths requiring documentation were changed."
  exit 0
fi

if [[ "$docs_updated" == true ]]; then
  echo "Documentation paths updated (operator-docs/ or README.md)."
  exit 0
fi

echo "::error::Operator-visible changes require updates to operator-docs/ or README.md, or #no-docs in the PR description with a reason."
exit 1
