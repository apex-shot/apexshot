#!/usr/bin/env bash
set -euo pipefail

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)

# The official Studio build configuration is optional for this local test
# package. When both variables are configured the package embeds the official
# public key and pinned release marker; when neither is, it builds without
# Studio unlock configuration and the free program still works. A half-set
# pair is a misconfiguration, not a silent downgrade, so it fails.
verify_key=${APEXSHOT_STUDIO_VERIFY_KEY:-}
release_timestamp=${APEXSHOT_STUDIO_RELEASE_TIMESTAMP:-}

if [ -n "$verify_key" ] || [ -n "$release_timestamp" ]; then
    : "${APEXSHOT_STUDIO_VERIFY_KEY:?Set the official public verification key}"
    : "${APEXSHOT_STUDIO_RELEASE_TIMESTAMP:?Set the pinned official Studio build marker}"

    python3 "$repo_root/scripts/release_gate.py" validate-config \
        "$APEXSHOT_STUDIO_VERIFY_KEY" "$APEXSHOT_STUDIO_RELEASE_TIMESTAMP"

    contents=$(printf 'APEXSHOT_STUDIO_VERIFY_KEY=%s\nAPEXSHOT_STUDIO_RELEASE_TIMESTAMP=%s\n' \
        "$APEXSHOT_STUDIO_VERIFY_KEY" "$APEXSHOT_STUDIO_RELEASE_TIMESTAMP")
else
    echo "No official Studio build configuration is configured; building without Studio unlock configuration."
    contents='# Built without official Studio unlock configuration.'
fi

temporary=$(mktemp "$repo_root/flatpak/.studio-build-config.XXXXXX")
trap 'rm -f "$temporary"' EXIT
umask 077
printf '%s\n' "$contents" > "$temporary"
chmod 600 "$temporary"
mv -f "$temporary" "$repo_root/flatpak/.studio-build-config.env"
