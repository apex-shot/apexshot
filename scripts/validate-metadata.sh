#!/usr/bin/env bash
# Validate desktop + AppStream metadata shipped in native packages.
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

desktop_files=(
    packaging/io.github.codegoddy.apexshot.desktop
    packaging/apexshot-daemon.desktop
)
metainfo=packaging/io.github.codegoddy.apexshot.metainfo.xml

for tool in desktop-file-validate appstreamcli; do
    if ! command -v "$tool" >/dev/null 2>&1; then
        echo "Missing required tool: $tool" >&2
        exit 1
    fi
done

for f in "${desktop_files[@]}"; do
    echo "desktop-file-validate $f"
    desktop-file-validate "$f"
done

echo "appstreamcli validate $metainfo"
appstreamcli validate "$metainfo"

echo "Metadata validation OK"
