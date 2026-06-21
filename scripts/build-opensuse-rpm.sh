#!/bin/bash
# shellcheck shell=bash
set -euo pipefail

SCRIPT_SOURCE="${BASH_SOURCE[0]:-}"
SCRIPT_DIR="$(cd "$(dirname "$SCRIPT_SOURCE")" && pwd)"
REPO_DIR="$(cd "${SCRIPT_DIR}/.." && pwd)"
SPEC_SRC="${REPO_DIR}/packaging/opensuse/apexshot.spec"
RPM_TOPDIR="${RPM_TOPDIR:-${REPO_DIR}/target/opensuse-rpmbuild}"

if ! command -v rpmbuild >/dev/null 2>&1; then
    echo "rpmbuild is required. Install it with: sudo zypper install rpm-build" >&2
    exit 1
fi

version="${1:-}"
if [[ -z "$version" ]]; then
    version="$(sed -n 's/^version = "\(.*\)"/\1/p' "${REPO_DIR}/Cargo.toml" | head -n 1)"
fi
version="${version#v}"

if [[ -z "$version" ]]; then
    echo "Could not determine package version" >&2
    exit 1
fi

mkdir -p "${RPM_TOPDIR}"/{BUILD,BUILDROOT,RPMS,SOURCES,SPECS,SRPMS}

archive="${RPM_TOPDIR}/SOURCES/apexshot-${version}.tar.gz"
git -C "$REPO_DIR" archive --format=tar.gz --prefix="apexshot-${version}/" -o "$archive" HEAD

spec="${RPM_TOPDIR}/SPECS/apexshot.spec"
sed "s/^Version:.*/Version:        ${version}/" "$SPEC_SRC" > "$spec"

rpmbuild \
    --define "_topdir ${RPM_TOPDIR}" \
    -ba "$spec"

echo "Built RPM artifacts under ${RPM_TOPDIR}/RPMS and ${RPM_TOPDIR}/SRPMS"
