#!/usr/bin/env bash
# Print mike alias flags for the tag being deployed.
set -euo pipefail

TAG="${1:?tag name required}"

git fetch --tags --force

if [[ "$TAG" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
  newest="$(git tag -l | grep -E '^[0-9]+\.[0-9]+\.[0-9]+$' | sort -V | tail -1)"
  if [[ "$TAG" == "$newest" ]]; then
    echo "MIKE_ALIAS_ARGS=--update-alias latest"
  else
    echo "MIKE_ALIAS_ARGS="
  fi
  if [[ "$TAG" =~ ^1\.[0-9]+\.[0-9]+$ ]]; then
    newest_1="$(git tag -l | grep -E '^1\.[0-9]+\.[0-9]+$' | sort -V | tail -1)"
    if [[ "$TAG" == "$newest_1" ]]; then
      echo "MIKE_STABLE_1_ARGS=--update-alias stable-1"
    else
      echo "MIKE_STABLE_1_ARGS="
    fi
  else
    echo "MIKE_STABLE_1_ARGS="
  fi
  exit 0
fi

if [[ "$TAG" =~ ^[0-9]+\.[0-9]+\.[0-9]+-dev\.[0-9]+$ ]]; then
  echo "MIKE_ALIAS_ARGS=--update-alias dev"
  echo "MIKE_STABLE_1_ARGS="
  exit 0
fi

echo "MIKE_ALIAS_ARGS="
echo "MIKE_STABLE_1_ARGS="
