#!/usr/bin/env bash
# Prepare packaging/arch for an AUR publish.
set -euo pipefail

usage() {
    cat <<'USAGE'
Usage: scripts/aur-prepare.sh <version-or-tag>

Examples:
  scripts/aur-prepare.sh v0.2.26
  scripts/aur-prepare.sh 0.2.26
USAGE
}

if [[ $# -ne 1 ]]; then
    usage >&2
    exit 2
fi

REPO="apex-shot/apexshot"
ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
ARCH_DIR="${ROOT_DIR}/packaging/arch"
PKGBUILD="${ARCH_DIR}/PKGBUILD"
SRCINFO="${ARCH_DIR}/.SRCINFO"

tag="$1"
pkgver="${tag#v}"

if [[ ! "$pkgver" =~ ^[0-9]+(\.[0-9]+)+([._+-][A-Za-z0-9]+)*$ ]]; then
    echo "Invalid version/tag: ${tag}" >&2
    exit 1
fi

source_url="https://github.com/${REPO}/archive/v${pkgver}.tar.gz"
archive_name="apexshot-${pkgver}.tar.gz"

tmpdir="$(mktemp -d -t apexshot-aur.XXXXXX)"
cleanup() {
    rm -rf "$tmpdir"
}
trap cleanup EXIT

echo "Downloading ${source_url}"
curl -fsSL "${source_url}" -o "${tmpdir}/${archive_name}"
sha256="$(sha256sum "${tmpdir}/${archive_name}" | awk '{print $1}')"

echo "Updating ${PKGBUILD}"
sed -i \
    -e "s/^pkgver=.*/pkgver=${pkgver}/" \
    -e "s#^source=.*#source=(\"${archive_name}::${source_url}\")#" \
    -e "s/^sha256sums=.*/sha256sums=('${sha256}')/" \
    "${PKGBUILD}"

if command -v makepkg >/dev/null 2>&1; then
    echo "Regenerating ${SRCINFO}"
    (
        cd "${ARCH_DIR}"
        makepkg --printsrcinfo > .SRCINFO
    )
else
    echo "makepkg not found; cannot regenerate .SRCINFO" >&2
    exit 1
fi

echo "Prepared AUR metadata for ${pkgver}"
