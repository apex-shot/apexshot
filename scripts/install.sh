#!/bin/bash
# Backward-compatible installer entrypoint.
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

refuse_steamos() {
    cat >&2 <<'EOF'
SteamOS is not supported by the ApexShot installer.

SteamOS is Arch-based, so this script looks like it should work. It will not:
the root filesystem is read-only, the desktop user has no sudo password until
you set one, and pacman keys are not initialized. SteamOS updates also wipe
anything pacman installs under /usr.

The flood of "corrupted / missing PGP" y/n prompts, and the final
"nothing was updated" / "no packages were upgraded" message, are pacman
hitting SteamOS's unsigned/stale package cache — not a broken ApexShot
package. Unlocking the filesystem and fixing keys will not give you a
lasting install.

If you want Steam Deck / SteamOS support, please open an issue:
https://github.com/apex-shot/apexshot/issues
EOF
    exit 1
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

    # SteamOS sets ID_LIKE=arch and ships pacman, but it is not Arch.
    if [[ "${id}" == "steamos" ]] || [[ -e /etc/steamos-release ]] || command -v steamos-readonly >/dev/null 2>&1; then
        printf '%s' "steamos"
        return
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
    steamos)
        refuse_steamos
        ;;
    arch)
        if [[ -n "$SCRIPT_DIR" && -f "${SCRIPT_DIR}/arch-install.sh" ]]; then
            exec bash "${SCRIPT_DIR}/arch-install.sh" "$@"
        fi
        exec bash -c "$(curl -fsSL https://raw.githubusercontent.com/apex-shot/apexshot/main/scripts/arch-install.sh)"
        ;;
    ubuntu)
        if [[ -n "$SCRIPT_DIR" && -f "${SCRIPT_DIR}/ubuntu-install.sh" ]]; then
            exec bash "${SCRIPT_DIR}/ubuntu-install.sh" "$@"
        fi
        exec bash -c "$(curl -fsSL https://raw.githubusercontent.com/apex-shot/apexshot/main/scripts/ubuntu-install.sh)"
        ;;
    fedora)
        if [[ -n "$SCRIPT_DIR" && -f "${SCRIPT_DIR}/fedora-install.sh" ]]; then
            exec bash "${SCRIPT_DIR}/fedora-install.sh" "$@"
        fi
        exec bash -c "$(curl -fsSL https://raw.githubusercontent.com/apex-shot/apexshot/main/scripts/fedora-install.sh)"
        ;;
    opensuse)
        echo "openSUSE binary packages are not published yet." >&2
        echo "Build locally: scripts/build-opensuse-rpm.sh && sudo zypper install target/opensuse-rpmbuild/RPMS/*/apexshot-*.rpm" >&2
        exit 1
        ;;
 esac

echo "Unsupported distribution: expected pacman, apt/dpkg, dnf, or zypper." >&2
exit 1
