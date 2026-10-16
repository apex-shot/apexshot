#!/usr/bin/env bash
set -euo pipefail

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
: "${APEXSHOT_STUDIO_VERIFY_KEY:?Set the official public verification key}"
: "${APEXSHOT_STUDIO_RELEASE_TIMESTAMP:?Set the pinned official Studio build marker}"

python3 "$repo_root/scripts/release_gate.py" validate-config \
    "$APEXSHOT_STUDIO_VERIFY_KEY" "$APEXSHOT_STUDIO_RELEASE_TIMESTAMP"

temporary=$(mktemp "$repo_root/flatpak/.studio-build-config.XXXXXX")
trap 'rm -f "$temporary"' EXIT
umask 077
printf 'APEXSHOT_STUDIO_VERIFY_KEY=%s\nAPEXSHOT_STUDIO_RELEASE_TIMESTAMP=%s\n' \
    "$APEXSHOT_STUDIO_VERIFY_KEY" "$APEXSHOT_STUDIO_RELEASE_TIMESTAMP" > "$temporary"
chmod 600 "$temporary"
mv -f "$temporary" "$repo_root/flatpak/.studio-build-config.env"
