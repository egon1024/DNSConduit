#!/usr/bin/env bash
# Build release tarballs, .deb packages, SHA256SUMS, and SBOM for a semver tag.
# Run from repository root. Requires: cargo, strip, tar, nfpm, sha256sum.
set -euo pipefail

VERSION="${VERSION:?VERSION is required (e.g. 0.14.0)}"
ARCH="${ARCH:-amd64}"
OUT_DIR="${OUT_DIR:-release-artifacts}"
NFPM="${NFPM:-nfpm}"

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$ROOT"

mkdir -p "$OUT_DIR"
rm -rf packaging/staging packaging/staging-dbg
mkdir -p packaging/staging/usr/bin packaging/staging-dbg/usr/bin

echo "Building release binaries for ${VERSION}..."
cargo build --release -p conduit -p conduitctl -p conduit-dnstap-tracer

BINARIES=(conduit conduitctl conduit-dnstap-tracer)
for bin in "${BINARIES[@]}"; do
  cp "target/release/${bin}" "packaging/staging-dbg/usr/bin/${bin}"
  cp "target/release/${bin}" "packaging/staging/usr/bin/${bin}"
  strip "packaging/staging/usr/bin/${bin}"
done

TARBALL_PROD="${OUT_DIR}/conduit-${VERSION}-${ARCH}.tar.gz"
TARBALL_DBG="${OUT_DIR}/conduit-${VERSION}-${ARCH}-debug.tar.gz"

pack_tarball() {
  local staging="$1"
  local output="$2"
  local tmp
  tmp="$(mktemp -d)"
  mkdir -p "${tmp}/conduit-${VERSION}"
  cp "${staging}/usr/bin/"* "${tmp}/conduit-${VERSION}/"
  cp LICENSE "${tmp}/conduit-${VERSION}/"
  mkdir -p "${tmp}/conduit-${VERSION}/examples"
  cp -a packaging/examples/. "${tmp}/conduit-${VERSION}/examples/"
  tar -C "${tmp}" -czf "${output}" "conduit-${VERSION}"
  rm -rf "${tmp}"
}

echo "Creating tarballs..."
pack_tarball packaging/staging "$TARBALL_PROD"
pack_tarball packaging/staging-dbg "$TARBALL_DBG"

export VERSION
echo "Building .deb packages..."
if ! command -v "$NFPM" >/dev/null 2>&1; then
  echo "::error::nfpm not found (set NFPM or install from https://nfpm.goreleaser.com)"
  exit 1
fi
"$NFPM" pkg --config packaging/nfpm/conduit.yaml --packager deb --target "$OUT_DIR"
"$NFPM" pkg --config packaging/nfpm/conduit-dbg.yaml --packager deb --target "$OUT_DIR"

DEB_PROD="${OUT_DIR}/conduit_${VERSION}_${ARCH}.deb"
DEB_DBG="${OUT_DIR}/conduit-dbg_${VERSION}_${ARCH}.deb"
# nfpm may emit slightly different names; normalize if needed
for f in "$OUT_DIR"/*.deb; do
  case "$(basename "$f")" in
    conduit_${VERSION}_*.deb)
      [[ "$f" != "$DEB_PROD" ]] && mv -f "$f" "$DEB_PROD"
      ;;
    conduit-dbg_${VERSION}_*.deb)
      [[ "$f" != "$DEB_DBG" ]] && mv -f "$f" "$DEB_DBG"
      ;;
  esac
done

echo "Generating SHA256SUMS..."
(
  cd "$OUT_DIR"
  sha256sum "$(basename "$TARBALL_PROD")" \
    "$(basename "$TARBALL_DBG")" \
    "$(basename "$DEB_PROD")" \
    "$(basename "$DEB_DBG")" >SHA256SUMS
)

SBOM_PATH="${OUT_DIR}/conduit-${VERSION}.spdx.json"
if cargo cyclonedx --version >/dev/null 2>&1; then
  echo "Generating SBOM..."
  cargo cyclonedx --manifest-path crates/conduit/Cargo.toml \
    --format json --all-features --describe crate -q
  mv "crates/conduit/conduit.cdx.json" "$SBOM_PATH"
  (
    cd "$OUT_DIR"
    sha256sum "$(basename "$SBOM_PATH")" >>SHA256SUMS
  )
else
  echo "cargo-cyclonedx not installed; skipping SBOM"
fi

echo "Release artifacts in ${OUT_DIR}:"
ls -la "$OUT_DIR"
