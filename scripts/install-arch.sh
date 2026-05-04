#!/bin/bash
# Backward-compatible Arch installer entrypoint.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
if [[ -f "${SCRIPT_DIR}/arch-install.sh" ]]; then
    exec bash "${SCRIPT_DIR}/arch-install.sh" "$@"
fi

exec bash -c "$(curl -fsSL https://raw.githubusercontent.com/apex-shot/apexshot/main/scripts/arch-install.sh)"
