#!/bin/bash
# shellcheck shell=bash
set -euo pipefail

# ============================================================================
# ApexShot Installer
# A stylish terminal UI for installing ApexShot and its dependencies.
# Usage: curl -fsSL https://raw.githubusercontent.com/apex-shot/apexshot/main/scripts/install.sh | bash
# ============================================================================

REPO="apex-shot/apexshot"
API_URL="https://api.github.com/repos/${REPO}"
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
    echo '    _                ____  _   _            _       '
    echo '   / \   _ __  _ __ / ___|| | | | __ _  ___| | _____ '
    echo '  / _ \ |  _ \|  _ \\___ \| |_| |/ _  |/ __| |/ / __|'
    echo ' / ___ \| |_) | |_) |__) |  _  | (_| | (__|   <\__ \ '
    echo '/_/   \_\  __/|  __/____/|_| |_|\__,_|\___|_|\_\___/'
    echo '       |_|   |_|                                      '
    echo -e "${RESET}"
    echo -e "${DIM}      Open-source Linux screen capture & recording tool${RESET}"
    echo -e "${DIM}      https://apexshot.org/${RESET}\n"
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

# --- Prerequisites -----------------------------------------------------------

check_prereqs() {
    step "Checking prerequisites"

    if command -v apt >/dev/null 2>&1; then
        ok "apt package manager found"
    else
        err "This installer currently supports Debian/Ubuntu (apt)."
        err "Please install manually or open an issue: https://github.com/${REPO}/issues"
        exit 1
    fi

    if command -v curl >/dev/null 2>&1; then
        ok "curl found"
    else
        warn "curl not found — installing via apt..."
        apt-get update -qq >/dev/null 2>&1
        apt-get install -y -qq curl >/dev/null 2>&1
        ok "curl installed"
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
        err "Root or sudo access is required to install packages."
        exit 1
    fi
}

# --- Fetch latest version ----------------------------------------------------

fetch_version() {
    step "Resolving latest release"
    VERSION=$(curl -fsSL "${API_URL}/releases/latest" | grep -o '"tag_name": *"[^"]*"' | head -n 1 | cut -d '"' -f 4)
    if [[ -z "$VERSION" ]]; then
        err "Could not determine the latest release version."
        err "Please check your internet connection or try again later."
        exit 1
    fi
    ok "Latest version: ${BOLD}${VERSION}${RESET}"
}

# --- Install system dependencies ---------------------------------------------

install_deps() {
    step "Installing system dependencies"

    info "This may take a few minutes..."

    local deps=(
        build-essential cmake pkg-config
        libx11-dev libxext6 libxtst-dev
        qtbase5-dev libqt5widgets5 libqt5x11extras5-dev libqt5network5-dev libqt5dbus5-dev
        libgstreamer1.0-dev gstreamer1.0-plugins-base gstreamer1.0-plugins-good
        gstreamer1.0-plugins-bad gstreamer1.0-plugins-ugly gstreamer1.0-libav
        libpipewire-0.3-dev
        tesseract-ocr tesseract-ocr-eng
        libgtk-4-dev libadwaita-1-dev libgtk4-layer-shell-dev
        wl-clipboard
        xdg-utils libnotify-bin ffmpeg unzip
        pipewire
    )

    # Some deps may already be present; skip if so to keep the UI clean.
    local missing=()
    for pkg in "${deps[@]}"; do
        if ! dpkg -s "$pkg" >/dev/null 2>&1; then
            missing+=("$pkg")
        fi
    done

    if [[ ${#missing[@]} -eq 0 ]]; then
        ok "All dependencies already satisfied"
        return
    fi

    info "Missing packages: ${missing[*]}"

    # Update apt quietly
    run_spinner "Updating package lists..." bash -c "${SUDO} apt-get update -qq"

    # Install missing packages quietly
    run_spinner "Installing missing packages..." bash -c "${SUDO} apt-get install -y -qq ${missing[*]}"

    ok "Dependencies installed"
}

# --- Download .deb -----------------------------------------------------------

download_deb() {
    step "Downloading ApexShot ${VERSION}"

    TMPDIR=$(mktemp -d -t apexshot-install.XXXXXX)
    local deb_url
    deb_url=$(curl -fsSL "${API_URL}/releases/latest" |
              grep "browser_download_url.*amd64.deb" |
              cut -d '"' -f 4)

    if [[ -z "$deb_url" ]]; then
        err "Could not find the .deb download URL."
        exit 1
    fi

    local deb_file="${TMPDIR}/apexshot_${VERSION}_amd64.deb"
    run_spinner "Downloading .deb package..." bash -c "curl -fsSL -o '${deb_file}' '${deb_url}'"

    ok "Package saved to ${deb_file}"
}

# --- Install .deb ------------------------------------------------------------

install_deb() {
    step "Installing ApexShot"

    local deb_file="${TMPDIR}/apexshot_${VERSION}_amd64.deb"
    run_spinner "Installing package..." bash -c "${SUDO} dpkg -i '${deb_file}' && ${SUDO} apt install -f -y -qq"

    ok "ApexShot installed"
}

# --- GNOME Extension ---------------------------------------------------------

install_gnome_extension() {
    step "Installing GNOME Shell extension"

    if ! command -v gnome-shell >/dev/null 2>&1; then
        warn "GNOME Shell not detected — skipping extension installation."
        info "The GNOME extension is required for full functionality on GNOME Wayland."
        return
    fi

    local shell_ver
    shell_ver=$(gnome-shell --version 2>/dev/null | awk '{print $3}' | cut -d. -f1)
    if [[ -z "$shell_ver" ]] || [[ "$shell_ver" -lt 45 ]] || [[ "$shell_ver" -gt 49 ]]; then
        warn "GNOME Shell version ${shell_ver:-unknown} is not in the supported range (45–49)."
        info "Skipping extension installation."
        return
    fi

    local zip_url
    zip_url=$(curl -fsSL "${API_URL}/releases" |
              grep -o '"browser_download_url": *"[^"]*apexshot-gnome-integration.zip"' |
              head -n 1 |
              cut -d '"' -f 4)

    if [[ -z "$zip_url" ]]; then
        warn "GNOME extension zip not found in releases — skipping."
        return
    fi

    local zip_file="${TMPDIR}/apexshot-gnome-integration.zip"
    run_spinner "Downloading GNOME extension..." bash -c "curl -fsSL -o '${zip_file}' '${zip_url}'"

    # Install for current user (no sudo needed for gnome-extensions)
    if command -v gnome-extensions >/dev/null 2>&1; then
        gnome-extensions install "${zip_file}" --force 2>/dev/null || true
        gnome-extensions enable apexshot-gnome-integration@apexshot.github.io 2>/dev/null || true
        ok "GNOME extension installed & enabled"
    else
        warn "gnome-extensions CLI not found — skipping automatic install."
        info "You can install it manually later: gnome-extensions install ${zip_file}"
    fi
}

# --- Browser native messaging host -------------------------------------------

setup_browser_host() {
    step "Browser integration"

    if [[ $EUID -ne 0 ]]; then
        info "Skipping browser native host setup (requires root)."
        info "Run 'sudo apexshot install --extension-id <id>' after installation if needed."
        return
    fi

    # The .deb already drops manifests into /etc/opt/chrome and /etc/chromium.
    # postinst copies them to user profiles. Nothing more to do here.
    ok "Browser native messaging host configured"
}

# --- Cleanup & summary -------------------------------------------------------

cleanup() {
    if [[ -n "${TMPDIR:-}" ]] && [[ -d "$TMPDIR" ]]; then
        rm -rf "$TMPDIR"
    fi
}

summary() {
    echo -e "\n${GREEN}${BOLD}═══════════════════════════════════════════════════════${RESET}"
    echo -e "${GREEN}${BOLD}  ApexShot is ready!${RESET}\n"
    echo -e "  Version:   ${BOLD}${VERSION}${RESET}"
    echo -e "  Binary:    ${BOLD}/usr/bin/apexshot${RESET}"
    echo -e "  Website:   ${DIM}https://apexshot.org/${RESET}"
    echo -e "  Issues:    ${DIM}https://github.com/${REPO}/issues${RESET}"
    echo -e "\n  ${BOLD}Quick start:${RESET}"
    echo -e "    apexshot capture screen    # Full-screen screenshot"
    echo -e "    apexshot capture area      # Area selection"
    echo -e "    apexshot record screen     # Start recording"
    echo -e "    apexshot settings          # Open settings"
    echo -e "\n  ${BOLD}Update later with:${RESET}"
    echo -e "    ${DIM}curl -fsSL https://raw.githubusercontent.com/${REPO}/main/scripts/update.sh | bash${RESET}"
    echo -e "${GREEN}${BOLD}═══════════════════════════════════════════════════════${RESET}\n"
}

# --- Main --------------------------------------------------------------------

main() {
    trap cleanup EXIT

    header
    check_prereqs
    fetch_version
    install_deps
    download_deb
    install_deb
    install_gnome_extension
    setup_browser_host
    summary
}

main "$@"
