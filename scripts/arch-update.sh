#!/bin/bash
# shellcheck shell=bash
set -euo pipefail

# ============================================================================
# ApexShot Arch Linux Updater
# A stylish terminal UI for updating ApexShot on Arch-based systems.
# Usage: curl -fsSL https://raw.githubusercontent.com/apex-shot/apexshot/main/scripts/arch-update.sh | bash
# ============================================================================

REPO="apex-shot/apexshot"
RELEASES_URL="https://github.com/${REPO}/releases"
EXT_UUID="apexshot-gnome-integration@apexshot.github.io"
VERSION=""
TMPDIR=""
SUDO=""

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

handoff_if_wrong_distro() {
    if ! command -v pacman >/dev/null 2>&1 && { command -v apt >/dev/null 2>&1 || command -v dpkg >/dev/null 2>&1; }; then
        echo "Ubuntu/Debian detected; switching to the Ubuntu/Debian updater."
        if [[ -f "${SCRIPT_DIR}/ubuntu-update.sh" ]]; then
            exec bash "${SCRIPT_DIR}/ubuntu-update.sh" "$@"
        fi
        exec bash -c "$(curl -fsSL https://raw.githubusercontent.com/${REPO}/main/scripts/ubuntu-update.sh)"
    fi
}

BOLD="\033[1m"
DIM="\033[2m"
RESET="\033[0m"
RED="\033[31m"
GREEN="\033[32m"
YELLOW="\033[33m"
BLUE="\033[34m"
CYAN="\033[36m"

header() {
    clear
    echo -e "${CYAN}${BOLD}"
    echo '    ___    ____  _______  _______ __  ______  ______'
    echo '   /   |  / __ \/ ____/ |/ / ___// / / / __ \/_  __/'
    echo '  / /| | / /_/ / __/  |   /\__ \/ /_/ / / / / / /   '
    echo ' / ___ |/ ____/ /___ /   |___/ / __  / /_/ / / /    '
    echo '/_/  |_/_/   /_____//_/|_/____/_/ /_/\____/ /_/     '
    echo -e "${RESET}"
    echo -e "${DIM}              Arch Linux update in progress...${RESET}\n"
}

step() {
    echo -e "\n${BLUE}${BOLD}▶${RESET} ${BOLD}$1${RESET}"
}

ok() {
    echo -e "  ${GREEN}✔${RESET}  $1"
}

warn() {
    echo -e "  ${YELLOW}⚠${RESET}  $1"
}

err() {
    echo -e "  ${RED}✖${RESET}  $1"
}

info() {
    echo -e "  ${DIM}$1${RESET}"
}

spinner() {
    local pid=$1
    local msg=$2
    local spin=('⠋' '⠙' '⠹' '⠸' '⠼' '⠴' '⠦' '⠧' '⠇' '⠏')
    local i=0
    while kill -0 "$pid" 2>/dev/null; do
        printf "\r  ${CYAN}%s${RESET} %s" "${spin[$i]}" "$msg"
        i=$(( (i + 1) % 10 ))
        sleep 0.08
    done
    printf "\r  ${GREEN}✔${RESET} %s\n" "$msg"
    wait "$pid"
}

run_spinner() {
    local msg=$1
    shift
    ("$@") &
    spinner $! "$msg"
}

download_file() {
    local url=$1
    local output=$2
    curl -fL --progress-bar -o "$output" "$url"
}

prime_sudo() {
    if [[ -n "$SUDO" ]]; then
        $SUDO -v
    fi
}

is_apexshot_binary() {
    local path=$1
    [[ -x "$path" ]] || return 1
    "$path" --version 2>/dev/null | grep -Eq '^apexshot [0-9]+\.[0-9]+\.[0-9]+'
}

move_shadowing_local_binary() {
    local name=$1
    local local_path="/usr/local/bin/${name}"
    local package_path="/usr/bin/${name}"

    [[ -e "$local_path" && -e "$package_path" ]] || return 0
    [[ "$local_path" -ef "$package_path" ]] && return 0
    [[ -x "$local_path" ]] || return 0

    if [[ "$name" == "apexshot" ]] && ! is_apexshot_binary "$local_path"; then
        warn "Leaving ${local_path} in place because it does not look like an ApexShot binary."
        return 0
    fi

    local backup="${local_path}.pre-package-update.$(date +%Y%m%d%H%M%S)"
    prime_sudo
    ${SUDO} mv "$local_path" "$backup"
    ok "Moved stale ${local_path} to ${backup}"
}

cleanup_shadowing_local_binaries() {
    step "Checking command path"

    move_shadowing_local_binary apexshot
    move_shadowing_local_binary apexshot-capture
    move_shadowing_local_binary apexshot-native-host

    hash -r 2>/dev/null || true
    if [[ "$(command -v apexshot 2>/dev/null || true)" != "/usr/bin/apexshot" ]]; then
        warn "apexshot still resolves to $(command -v apexshot 2>/dev/null || echo '<not found>')"
        info "Check your PATH for another stale ApexShot binary."
    else
        ok "apexshot now resolves to /usr/bin/apexshot"
    fi
}

run_gnome_extensions() {
    if [[ $EUID -eq 0 ]] && [[ -n "${SUDO_USER:-}" ]] && [[ "${SUDO_USER}" != "root" ]]; then
        local target_uid runtime_dir bus_address
        target_uid=$(id -u "${SUDO_USER}" 2>/dev/null || true)
        if [[ -n "$target_uid" ]]; then
            runtime_dir="${XDG_RUNTIME_DIR:-/run/user/${target_uid}}"
            bus_address="${DBUS_SESSION_BUS_ADDRESS:-unix:path=${runtime_dir}/bus}"
            if command -v sudo >/dev/null 2>&1; then
                sudo -u "${SUDO_USER}" env XDG_RUNTIME_DIR="${runtime_dir}" DBUS_SESSION_BUS_ADDRESS="${bus_address}" gnome-extensions "$@"
            else
                runuser -u "${SUDO_USER}" -- env XDG_RUNTIME_DIR="${runtime_dir}" DBUS_SESSION_BUS_ADDRESS="${bus_address}" gnome-extensions "$@"
            fi
            return
        fi
    fi

    gnome-extensions "$@"
}

install_gnome_extension_files() {
    local zip_file=$1

    if ! command -v unzip >/dev/null 2>&1; then
        return 1
    fi

    local target_user="" target_home="${HOME:-}" target_uid="" target_gid=""
    if [[ $EUID -eq 0 ]] && [[ -n "${SUDO_USER:-}" ]] && [[ "${SUDO_USER}" != "root" ]]; then
        target_user="${SUDO_USER}"
        target_home=$(getent passwd "${target_user}" 2>/dev/null | cut -d: -f6)
        target_uid=$(id -u "${target_user}" 2>/dev/null || true)
        target_gid=$(id -g "${target_user}" 2>/dev/null || true)
    fi

    if [[ -z "$target_home" ]]; then
        return 1
    fi

    local ext_parent="${target_home}/.local/share/gnome-shell/extensions"
    local ext_dir="${ext_parent}/${EXT_UUID}"
    rm -rf "${ext_dir}"
    mkdir -p "${ext_dir}"
    unzip -q "${zip_file}" -d "${ext_dir}"

    if [[ -n "$target_uid" ]] && [[ -n "$target_gid" ]]; then
        chown -R "${target_uid}:${target_gid}" "${ext_dir}"
    fi
}

cleanup() {
    if [[ -n "${TMPDIR:-}" ]] && [[ -d "$TMPDIR" ]]; then
        rm -rf "$TMPDIR"
    fi
}

check_prereqs() {
    step "Checking prerequisites"

    if command -v pacman >/dev/null 2>&1; then
        ok "pacman package manager found"
    else
        err "This updater is designed for Arch Linux and pacman-based systems."
        exit 1
    fi

    if [[ $EUID -eq 0 ]]; then
        SUDO=""
    elif command -v sudo >/dev/null 2>&1; then
        SUDO="sudo"
    else
        err "Root or sudo access is required to update packages."
        exit 1
    fi

    if command -v curl >/dev/null 2>&1; then
        ok "curl found"
    else
        warn "curl not found — installing via pacman..."
        prime_sudo
        run_spinner "Installing curl..." bash -c "${SUDO} pacman -S --noconfirm curl"
        ok "curl installed"
    fi
}

detect_current_version() {
    step "Detecting installed version"

    local current=""
    if command -v apexshot >/dev/null 2>&1; then
        current=$(apexshot --version 2>/dev/null || echo "")
    fi

    if [[ -n "$current" ]]; then
        ok "Currently installed: ${BOLD}${current}${RESET}"
    else
        warn "Could not detect current ApexShot version"
        info "Continuing with package update..."
    fi
}

fetch_version() {
    step "Resolving latest release"

    local effective
    effective=$(curl -fsSLI -o /dev/null -w '%{url_effective}' "${RELEASES_URL}/latest" || true)
    VERSION="${effective##*/}"
    if [[ -z "$VERSION" ]] || [[ "$VERSION" == "latest" ]]; then
        err "Could not determine the latest release version."
        err "Please check your internet connection or try again later."
        exit 1
    fi
    ok "Latest version: ${BOLD}${VERSION}${RESET}"
}

update_from_release() {
    step "Updating from GitHub release"

    fetch_version
    TMPDIR=$(mktemp -d -t apexshot-arch-update.XXXXXX)

    local pkg_path
    pkg_path=$(curl -fsSL "${RELEASES_URL}/expanded_assets/${VERSION}" |
               grep -oE "/${REPO}/releases/download/${VERSION}/[^\"]*x86_64\.pkg\.tar\.zst" |
               head -n 1)
    local pkg_url=""
    [[ -n "$pkg_path" ]] && pkg_url="https://github.com${pkg_path}"

    if [[ -z "$pkg_url" ]]; then
        err "Could not find the Arch package download URL."
        exit 1
    fi

    local pkg_file="${TMPDIR}/apexshot_${VERSION}_x86_64.pkg.tar.zst"
    info "Downloading Arch package with progress:"
    download_file "$pkg_url" "$pkg_file"
    ok "Package saved to ${pkg_file}"

    info "Stopping any running ApexShot daemon..."
    killall -9 apexshot 2>/dev/null || true

    prime_sudo
    info "Installing package with pacman:"
    ${SUDO} pacman -U --noconfirm --overwrite 'usr/share/apexshot/sounds/*.ogg' "${pkg_file}"
    ok "ApexShot updated to ${VERSION}"
}

update_gnome_extension() {
    step "Updating GNOME Shell extension"

    if ! command -v gnome-extensions >/dev/null 2>&1; then
        warn "gnome-extensions CLI not found - package files were updated, but the active extension could not be refreshed."
        info "Install gnome-shell and run: gnome-extensions enable ${EXT_UUID}"
        return
    fi

    local zip_path
    zip_path=$(curl -fsSL "${RELEASES_URL}/expanded_assets/${VERSION}" |
               grep -oE "/${REPO}/releases/download/${VERSION}/[^\"]*apexshot-gnome-integration\.zip" |
               head -n 1)
    local zip_url=""
    [[ -n "$zip_path" ]] && zip_url="https://github.com${zip_path}"

    if [[ -z "$zip_url" ]]; then
        warn "GNOME extension zip not found in releases - using package-installed extension files only."
        run_gnome_extensions enable "${EXT_UUID}" 2>/dev/null || true
        return
    fi

    local zip_file="${TMPDIR}/apexshot-gnome-integration.zip"
    info "Downloading GNOME extension with progress:"
    download_file "$zip_url" "$zip_file"

    local was_enabled=0
    if run_gnome_extensions list --enabled 2>/dev/null | grep -Fxq "${EXT_UUID}"; then
        was_enabled=1
    fi

    run_gnome_extensions disable "${EXT_UUID}" >/dev/null 2>&1 || true
    if ! run_gnome_extensions install --force "${zip_file}" >/dev/null 2>&1; then
        warn "gnome-extensions install failed - replacing user extension files directly."
        if ! install_gnome_extension_files "${zip_file}"; then
            if [[ $was_enabled -eq 1 ]]; then
                run_gnome_extensions enable "${EXT_UUID}" >/dev/null 2>&1 || true
            fi
            err "Could not install the GNOME extension update."
            err "Try logging out and back in, then run this updater again."
            exit 1
        fi
    fi

    if run_gnome_extensions enable "${EXT_UUID}" >/dev/null 2>&1; then
        ok "GNOME extension updated and enabled"
    else
        warn "GNOME extension files were updated, but GNOME could not enable it in this session."
        info "Log out and back in, then run: gnome-extensions enable ${EXT_UUID}"
    fi
}

summary() {
    echo -e "\n${GREEN}${BOLD}═══════════════════════════════════════════════════════${RESET}"
    echo -e "${GREEN}${BOLD}  ApexShot is up to date!${RESET}\n"
    echo -e "  Version:   ${BOLD}${VERSION}${RESET}"
    echo -e "  Binary:    ${BOLD}/usr/bin/apexshot${RESET}"
    echo -e "\n  ${BOLD}Quick start:${RESET}"
    echo -e "    apexshot capture screen    # Full-screen screenshot"
    echo -e "    apexshot capture area      # Area selection"
    echo -e "    apexshot record screen     # Start recording"
    echo -e "    apexshot settings          # Open settings"
    echo -e "${GREEN}${BOLD}═══════════════════════════════════════════════════════${RESET}\n"
}

main() {
    trap cleanup EXIT

    handoff_if_wrong_distro "$@"
    header
    check_prereqs
    detect_current_version
    update_from_release
    cleanup_shadowing_local_binaries
    update_gnome_extension
    summary
}

main "$@"
