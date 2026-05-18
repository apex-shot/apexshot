#!/bin/bash
# Backward-compatible updater entrypoint.
set -euo pipefail

SCRIPT_SOURCE="${BASH_SOURCE[0]:-}"
SCRIPT_DIR=""
if [[ -n "$SCRIPT_SOURCE" ]]; then
    SCRIPT_DIR="$(cd "$(dirname "$SCRIPT_SOURCE")" && pwd)"
fi

is_gnome_session() {
    local desktop="${XDG_CURRENT_DESKTOP:-}:${XDG_SESSION_DESKTOP:-}:${DESKTOP_SESSION:-}"
    [[ -n "${GNOME_SETUP_DISPLAY:-}" ]] || [[ "${desktop,,}" == *gnome* ]]
}

if ! is_gnome_session; then
    export APEXSHOT_SKIP_GNOME_EXTENSION=1
fi

if command -v pacman >/dev/null 2>&1; then
    if [[ -n "$SCRIPT_DIR" && -f "${SCRIPT_DIR}/arch-update.sh" ]]; then
        exec bash "${SCRIPT_DIR}/arch-update.sh" "$@"
    fi
    exec bash -c "$(curl -fsSL https://raw.githubusercontent.com/apex-shot/apexshot/main/scripts/arch-update.sh)"
fi

if command -v apt >/dev/null 2>&1 || command -v dpkg >/dev/null 2>&1; then
    if [[ -n "$SCRIPT_DIR" && -f "${SCRIPT_DIR}/ubuntu-update.sh" ]]; then
        exec bash "${SCRIPT_DIR}/ubuntu-update.sh" "$@"
    fi
    exec bash -c "$(curl -fsSL https://raw.githubusercontent.com/apex-shot/apexshot/main/scripts/ubuntu-update.sh)"
fi

echo "Unsupported distribution: expected pacman, apt, or dpkg." >&2
exit 1
