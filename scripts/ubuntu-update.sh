#!/bin/bash
# shellcheck shell=bash
set -euo pipefail

# ============================================================================
# ApexShot Ubuntu/Debian Updater
# A stylish terminal UI for updating ApexShot to the latest release.
# Usage: curl -fsSL https://raw.githubusercontent.com/apex-shot/apexshot/main/scripts/ubuntu-update.sh | bash
# ============================================================================

REPO="apex-shot/apexshot"
RELEASES_URL="https://github.com/${REPO}/releases"
VERSION=""
TMPDIR=""
SUDO=""

# --- ANSI colours & styles ---------------------------------------------------
BOLD="\033[1m"
DIM="\033[2m"
RESET="\033[0m"
RED="\033[31m"
GREEN="\033[32m"
YELLOW="\033[33m"
BLUE="\033[34m"
CYAN="\033[36m"
MAGENTA="\033[35m"
WHITE="\033[37m"

# --- UI helpers --------------------------------------------------------------

header() {
    clear
    echo -e "${CYAN}${BOLD}"
    echo '    ___    ____  _______  _______ __  ______  ______'
    echo '   /   |  / __ \/ ____/ |/ / ___// / / / __ \/_  __/'
    echo '  / /| | / /_/ / __/  |   /\__ \/ /_/ / / / / / /   '
    echo ' / ___ |/ ____/ /___ /   |___/ / __  / /_/ / / /    '
    echo '/_/  |_/_/   /_____//_/|_/____/_/ /_/\____/ /_/     '
    echo -e "${RESET}"
    echo -e "${DIM}                 Update in progress...${RESET}\n"
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

# Prompt the user for their sudo password up front so the subsequent
# commands inside a spinner don't have their prompt clobbered by the
# spinner output. No-op when running as root.
prime_sudo() {
    if [[ -n "$SUDO" ]]; then
        $SUDO -v
    fi
}

# --- Prerequisites -----------------------------------------------------------

check_prereqs() {
    step "Checking prerequisites"

    if command -v curl >/dev/null 2>&1; then
        ok "curl found"
    else
        err "curl is required but not found."
        err "Install it with: sudo apt install curl"
        exit 1
    fi

    if command -v dpkg >/dev/null 2>&1; then
        ok "dpkg found"
    else
        err "dpkg is required but not found."
        exit 1
    fi

    # Resolve sudo or fall back to root
    if [[ $EUID -eq 0 ]]; then
        SUDO=""
    elif command -v sudo >/dev/null 2>&1; then
        SUDO="sudo"
    else
        err "Root or sudo access is required to update packages."
        exit 1
    fi
}

# --- Detect current version --------------------------------------------------

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
        info "Is ApexShot installed? Continuing anyway..."
    fi
}

# --- Fetch latest version ----------------------------------------------------

fetch_version() {
    step "Resolving latest release"

    # Use the public redirect from /releases/latest -> /releases/tag/<TAG>.
    # This avoids api.github.com which is rate-limited (60 req/hour per IP
    # for unauthenticated callers).
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

# --- Download latest .deb ----------------------------------------------------

download_latest() {
    step "Downloading ApexShot ${VERSION}"

    TMPDIR=$(mktemp -d -t apexshot-update.XXXXXX)
    # Scrape the release's assets page (HTML, not API) to find the .deb URL.
    local deb_path
    deb_path=$(curl -fsSL "${RELEASES_URL}/expanded_assets/${VERSION}" |
               grep -oE "/${REPO}/releases/download/${VERSION}/[^\"]*amd64\.deb" |
               head -n 1)
    local deb_url=""
    [[ -n "$deb_path" ]] && deb_url="https://github.com${deb_path}"

    if [[ -z "$deb_url" ]]; then
        err "Could not find the .deb download URL."
        exit 1
    fi

    local deb_file="${TMPDIR}/apexshot_${VERSION}_amd64.deb"
    info "Downloading .deb package with progress:"
    download_file "$deb_url" "$deb_file"

    ok "Package saved to ${deb_file}"
}

# --- Install update ----------------------------------------------------------

install_update() {
    step "Installing update"

    local deb_file="${TMPDIR}/apexshot_${VERSION}_amd64.deb"

    # Kill running apexshot daemon to avoid file locks
    info "Stopping any running ApexShot daemon..."
    killall -9 apexshot 2>/dev/null || true

    prime_sudo
    run_spinner "Upgrading package..." bash -c "${SUDO} dpkg -i '${deb_file}' && ${SUDO} apt install -f -y -qq"

    ok "ApexShot updated to ${VERSION}"
}

# --- Update GNOME Extension --------------------------------------------------

update_gnome_extension() {
    step "Updating GNOME Shell extension"

    if ! command -v gnome-shell >/dev/null 2>&1; then
        warn "GNOME Shell not detected — skipping extension update."
        return
    fi

    if ! command -v gnome-extensions >/dev/null 2>&1; then
        warn "gnome-extensions CLI not found — skipping extension update."
        return
    fi

    local zip_path
    zip_path=$(curl -fsSL "${RELEASES_URL}/expanded_assets/${VERSION}" |
               grep -oE "/${REPO}/releases/download/${VERSION}/[^\"]*apexshot-gnome-integration\.zip" |
               head -n 1)
    local zip_url=""
    [[ -n "$zip_path" ]] && zip_url="https://github.com${zip_path}"

    if [[ -z "$zip_url" ]]; then
        warn "GNOME extension zip not found in releases — skipping."
        return
    fi

    local zip_file="${TMPDIR}/apexshot-gnome-integration.zip"
    info "Downloading GNOME extension with progress:"
    download_file "$zip_url" "$zip_file"

    gnome-extensions install "${zip_file}" --force 2>/dev/null || true
    gnome-extensions enable apexshot-gnome-integration@apexshot.github.io 2>/dev/null || true
    ok "GNOME extension updated & enabled"
}

# --- Cleanup & summary -------------------------------------------------------

cleanup() {
    if [[ -n "${TMPDIR:-}" ]] && [[ -d "$TMPDIR" ]]; then
        rm -rf "$TMPDIR"
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

# --- Main --------------------------------------------------------------------

main() {
    trap cleanup EXIT

    header
    check_prereqs
    detect_current_version
    fetch_version
    download_latest
    install_update
    update_gnome_extension
    summary
}

main "$@"
