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

detect_distro_family() {
    local id=""
    local id_like=""

    if [[ -r /etc/os-release ]]; then
        # shellcheck disable=SC1091
        source /etc/os-release
        id="${ID:-}"
        id_like="${ID_LIKE:-}"
    fi

    case " ${id} ${id_like} " in
        *" arch "*|*" manjaro "*)
            printf '%s' "arch"
            return
            ;;
        *" fedora "*|*" rhel "*|*" centos "*|*" rocky "*|*" alma "*)
            printf '%s' "fedora"
            return
            ;;
        *" opensuse "*|*" suse "*|*" sles "*)
            printf '%s' "opensuse"
            return
            ;;
        *" debian "*|*" ubuntu "*|*" pop "*|*" linuxmint "*)
            printf '%s' "ubuntu"
            return
            ;;
    esac

    if command -v pacman >/dev/null 2>&1; then
        printf '%s' "arch"
    elif command -v dnf >/dev/null 2>&1; then
        printf '%s' "fedora"
    elif command -v zypper >/dev/null 2>&1; then
        printf '%s' "opensuse"
    elif command -v apt >/dev/null 2>&1 || command -v dpkg >/dev/null 2>&1; then
        printf '%s' "ubuntu"
    else
        printf '%s' "unknown"
    fi
}

if ! is_gnome_session; then
    export APEXSHOT_SKIP_GNOME_EXTENSION=1
fi

case "$(detect_distro_family)" in
    arch)
        if [[ -n "$SCRIPT_DIR" && -f "${SCRIPT_DIR}/arch-update.sh" ]]; then
            exec bash "${SCRIPT_DIR}/arch-update.sh" "$@"
        fi
        exec bash -c "$(curl -fsSL https://raw.githubusercontent.com/apex-shot/apexshot/main/scripts/arch-update.sh)"
        ;;
    ubuntu)
        if [[ -n "$SCRIPT_DIR" && -f "${SCRIPT_DIR}/ubuntu-update.sh" ]]; then
            exec bash "${SCRIPT_DIR}/ubuntu-update.sh" "$@"
        fi
        exec bash -c "$(curl -fsSL https://raw.githubusercontent.com/apex-shot/apexshot/main/scripts/ubuntu-update.sh)"
        ;;
    fedora)
        if [[ -n "$SCRIPT_DIR" && -f "${SCRIPT_DIR}/fedora-update.sh" ]]; then
            exec bash "${SCRIPT_DIR}/fedora-update.sh" "$@"
        fi
        exec bash -c "$(curl -fsSL https://raw.githubusercontent.com/apex-shot/apexshot/main/scripts/fedora-update.sh)"
        ;;
    opensuse)
        if [[ -n "$SCRIPT_DIR" && -f "${SCRIPT_DIR}/opensuse-update.sh" ]]; then
            exec bash "${SCRIPT_DIR}/opensuse-update.sh" "$@"
        fi
        exec bash -c "$(curl -fsSL https://raw.githubusercontent.com/apex-shot/apexshot/main/scripts/opensuse-update.sh)"
        ;;
 esac

echo "Unsupported distribution: expected pacman, apt/dpkg, dnf, or zypper." >&2
exit 1
