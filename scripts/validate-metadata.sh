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

# --no-net keeps validation deterministic: the URL/screenshot reachability
# checks hit GitHub and fail on transient 5xx responses (the bugtracker URL
# has flaked with a 504), turning a metadata-structure check into a flaky
# network probe. We validate the file's content, not the network's uptime.
echo "appstreamcli validate --no-net $metainfo"
appstreamcli validate --no-net "$metainfo"

echo "Metadata validation OK"
