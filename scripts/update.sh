#!/bin/bash
# Backward-compatible updater entrypoint.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

if command -v pacman >/dev/null 2>&1; then
    if [[ -f "${SCRIPT_DIR}/arch-update.sh" ]]; then
        exec bash "${SCRIPT_DIR}/arch-update.sh" "$@"
    fi
    exec bash -c "$(curl -fsSL https://raw.githubusercontent.com/apex-shot/apexshot/main/scripts/arch-update.sh)"
fi

if command -v apt >/dev/null 2>&1 || command -v dpkg >/dev/null 2>&1; then
    if [[ -f "${SCRIPT_DIR}/ubuntu-update.sh" ]]; then
        exec bash "${SCRIPT_DIR}/ubuntu-update.sh" "$@"
    fi
    exec bash -c "$(curl -fsSL https://raw.githubusercontent.com/apex-shot/apexshot/main/scripts/ubuntu-update.sh)"
fi

echo "Unsupported distribution: expected pacman, apt, or dpkg." >&2
exit 1
