#!/bin/bash
# Backward-compatible Arch installer entrypoint.
set -euo pipefail

SCRIPT_SOURCE="${BASH_SOURCE[0]:-}"
SCRIPT_DIR=""
if [[ -n "$SCRIPT_SOURCE" ]]; then
    SCRIPT_DIR="$(cd "$(dirname "$SCRIPT_SOURCE")" && pwd)"
fi
if [[ -n "$SCRIPT_DIR" && -f "${SCRIPT_DIR}/arch-install.sh" ]]; then
    exec bash "${SCRIPT_DIR}/arch-install.sh" "$@"
fi

exec bash -c "$(curl -fsSL https://raw.githubusercontent.com/apex-shot/apexshot/main/scripts/arch-install.sh)"
