#!/usr/bin/env bash
# Local preview for the global Versions page (site-root/versions/index.html).
#
# Builds a throwaway directory tree that mirrors gh-pages layout, optionally
# fetches the live versions.json, and serves HTTP on a local port. Nothing is
# committed or published.
#
# Usage (from repository root):
#   operator-docs/scripts/preview-versions-page.sh
#   operator-docs/scripts/preview-versions-page.sh --port 9000
#   operator-docs/scripts/preview-versions-page.sh --offline   # bundled mock versions.json
#
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
SITE_ROOT="${ROOT}/operator-docs/site-root"
PORT=8765
OFFLINE=0

while [[ $# -gt 0 ]]; do
  case "$1" in
    --port)
      PORT="${2:?missing port value}"
      shift 2
      ;;
    --offline)
      OFFLINE=1
      shift
      ;;
    -h|--help)
      sed -n '2,12p' "$0"
      exit 0
      ;;
    *)
      echo "Unknown option: $1" >&2
      exit 2
      ;;
  esac
done

PREVIEW="$(mktemp -d)"
cleanup() { rm -rf "${PREVIEW}"; }
trap cleanup EXIT

mkdir -p "${PREVIEW}/versions"
cp "${SITE_ROOT}/versions/index.html" "${PREVIEW}/versions/index.html"
cp "${SITE_ROOT}/versions/release-dates.json" "${PREVIEW}/versions/release-dates.json"

if [[ "${OFFLINE}" -eq 1 ]]; then
  cat >"${PREVIEW}/versions.json" <<'EOF'
[
  {"version": "0.18.0", "title": "0.18.0", "aliases": ["latest"]},
  {"version": "0.17.0", "title": "0.17.0", "aliases": []},
  {"version": "0.16.0", "title": "0.16.0", "aliases": []},
  {"version": "0.15.0", "title": "0.15.0", "aliases": []},
  {"version": "0.14.0", "title": "0.14.0", "aliases": []},
  {"version": "0.13.0", "title": "0.13.0", "aliases": []},
  {"version": "0.12.0", "title": "0.12.0", "aliases": []}
]
EOF
else
  if ! curl -fsSL "https://egon1024.github.io/DNSConduit/versions.json" \
    -o "${PREVIEW}/versions.json"; then
    echo "Could not fetch live versions.json; re-run with --offline" >&2
    exit 1
  fi
fi

URL="http://127.0.0.1:${PORT}/versions/"
echo "Serving Versions page preview at ${URL}"
echo "Press Ctrl+C to stop."
echo "Edit operator-docs/site-root/versions/release-dates.json and refresh the browser."
cd "${PREVIEW}"
exec python3 -m http.server "${PORT}" --bind 127.0.0.1
