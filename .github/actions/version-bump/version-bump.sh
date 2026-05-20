#!/usr/bin/env bash
# Compute next semver from latest GitHub release and PR description directives.
set -euo pipefail

PR_BODY="${PR_BODY:-}"
DEFAULT_BUMP="${DEFAULT_BUMP:-minor}"

write_output() {
  local key="$1" value="$2"
  {
    echo "${key}=${value}"
  } >>"${GITHUB_OUTPUT:?GITHUB_OUTPUT is not set}"
}

fail_with_error() {
  local code="$1"
  write_output error "$code"
  write_output current_version ""
  write_output next_version ""
  write_output bump_level ""
  write_output bump_source ""
  exit 1
}

normalize_tag() {
  local tag="$1"
  tag="${tag#v}"
  tag="${tag#V}"
  echo "$tag"
}

is_valid_semver() {
  [[ "$1" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]
}

bump_semver() {
  local version="$1" level="$2"
  local major minor patch
  IFS=. read -r major minor patch <<<"$version"
  case "$level" in
    major) echo "$((major + 1)).0.0" ;;
    minor) echo "${major}.$((minor + 1)).0" ;;
    patch) echo "${major}.${minor}.$((patch + 1))" ;;
    *) return 1 ;;
  esac
}

current_version="0.0.0"
if latest_tag="$(
  gh release list --limit 1 --json tagName --jq '.[0].tagName // empty' 2>/dev/null
)" && [[ -n "$latest_tag" ]]; then
  current_version="$(normalize_tag "$latest_tag")"
fi

if ! is_valid_semver "$current_version"; then
  echo "::error::Latest release tag is not valid semver: ${current_version}"
  fail_with_error invalid_current_version
fi

declare -a directives=()
if [[ -n "$PR_BODY" ]]; then
  while IFS= read -r line || [[ -n "$line" ]]; do
    # Trim leading/trailing whitespace
    line="${line#"${line%%[![:space:]]*}"}"
    line="${line%"${line##*[![:space:]]}"}"
    shopt -s nocasematch
    case "$line" in
      '#major') directives+=('major') ;;
      '#minor') directives+=('minor') ;;
      '#patch') directives+=('patch') ;;
    esac
    shopt -u nocasematch
  done <<<"$PR_BODY"
fi

declare -A seen_levels=()
bump_level=""
for level in "${directives[@]}"; do
  seen_levels[$level]=1
done

distinct_count=${#seen_levels[@]}
if ((distinct_count > 1)); then
  echo "::error::Conflicting semver directives in PR description (#major, #minor, #patch)"
  fail_with_error conflicting_directives
fi

bump_source="implicit"
if ((distinct_count == 1)); then
  for level in "${!seen_levels[@]}"; do
    bump_level="$level"
  done
  bump_source="explicit"
else
  bump_level="$DEFAULT_BUMP"
fi

case "$bump_level" in
  major | minor | patch) ;;
  *)
    echo "::error::Invalid default bump: ${bump_level}"
    fail_with_error invalid_default_bump
    ;;
esac

next_version="$(bump_semver "$current_version" "$bump_level")"

write_output error ""
write_output current_version "$current_version"
write_output next_version "$next_version"
write_output bump_level "$bump_level"
write_output bump_source "$bump_source"

echo "Current: ${current_version} -> Next: ${next_version} (${bump_level}, ${bump_source})"
