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
VERSION=""
TMPDIR=""
SUDO=""

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

update_with_aur_helper() {
    local aur_helper=""
    for helper in yay paru trizen pikaur; do
        if command -v "$helper" >/dev/null 2>&1; then
            aur_helper="$helper"
            break
        fi
    done

    if [[ -z "$aur_helper" ]]; then
        return 1
    fi

    step "Updating from AUR"
    ok "Found AUR helper: ${aur_helper}"
    run_spinner "Updating apexshot via ${aur_helper}..." bash -c "${aur_helper} -Syu --noconfirm apexshot"
    return 0
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

    info "Stopping any running ApexShot daemon..."
    killall -9 apexshot 2>/dev/null || true

    prime_sudo
    run_spinner "Upgrading package..." bash -c "${SUDO} pacman -U --noconfirm '${pkg_file}'"
    ok "ApexShot updated to ${VERSION}"
}

summary() {
    echo -e "\n${GREEN}${BOLD}═══════════════════════════════════════════════════════${RESET}"
    echo -e "${GREEN}${BOLD}  ApexShot is up to date!${RESET}\n"
    echo -e "  Version:   ${BOLD}${VERSION:-AUR latest}${RESET}"
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

    header
    check_prereqs
    detect_current_version
    if ! update_with_aur_helper; then
        update_from_release
    fi
    summary
}

main "$@"
