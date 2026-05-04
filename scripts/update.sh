#!/bin/bash
# Backward-compatible Ubuntu/Debian updater entrypoint.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
if [[ -f "${SCRIPT_DIR}/ubuntu-update.sh" ]]; then
    exec bash "${SCRIPT_DIR}/ubuntu-update.sh" "$@"
fi

exec bash -c "$(curl -fsSL https://raw.githubusercontent.com/apex-shot/apexshot/main/scripts/ubuntu-update.sh)"
