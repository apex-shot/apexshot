#!/bin/bash
# shellcheck shell=bash
set -euo pipefail

# ============================================================================
# ApexShot Installer for Arch Linux
# A stylish terminal UI for installing ApexShot on Arch-based systems.
# Usage: curl -fsSL https://raw.githubusercontent.com/apex-shot/apexshot/main/scripts/install-arch.sh | bash
# ============================================================================

REPO="apex-shot/apexshot"
RELEASES_URL="https://github.com/${REPO}/releases"
AUR_URL="https://aur.archlinux.org/packages/apexshot"
VERSION=""
TMPDIR=""
SUDO=""
INSTALL_PATH="/usr/bin/apexshot"

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
    echo -e "${DIM}      Open-source Linux screen capture & recording tool${RESET}"
    echo -e "${DIM}      Arch Linux Edition${RESET}\n"
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

prime_sudo() {
    if [[ -n "$SUDO" ]]; then
        $SUDO -v
    fi
}

# --- Prerequisites -----------------------------------------------------------

check_prereqs() {
    step "Checking prerequisites"

    # Arch-specific: Check for pacman
    if command -v pacman >/dev/null 2>&1; then
        ok "pacman package manager found"
    else
        err "This installer is designed for Arch Linux and pacman-based systems."
        err "For other distributions, please use the appropriate installer."
        exit 1
    fi

    # Check for base-devel (required for AUR builds)
    if pacman -Qq base-devel >/dev/null 2>&1; then
        ok "base-devel group found"
    else
        warn "base-devel group not found - required for building from AUR"
    fi

    if command -v curl >/dev/null 2>&1; then
        ok "curl found"
    else
        warn "curl not found — installing via pacman..."
        prime_sudo
        run_spinner "Installing curl..." bash -c "${SUDO} pacman -S --noconfirm curl"
        ok "curl installed"
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

# --- Installation method selection ------------------------------------------

select_install_method() {
    step "Select installation method"
    
    echo "  1) Install from AUR (recommended - auto-updates)"
    echo "  2) Install pre-built binary from GitHub releases"
    echo "  3) Build from source"
    echo ""
    
    # Default to AUR if no input
    read -rp "  Select [1-3] (default: 1): " choice
    choice=${choice:-1}
    
    case $choice in
        1) install_from_aur ;;
        2) install_from_release ;;
        3) install_from_source ;;
        *) warn "Invalid choice, defaulting to AUR"; install_from_aur ;;
    esac
}

# --- Install from AUR --------------------------------------------------------

install_from_aur() {
    step "Installing from AUR"
    
    info "Checking for AUR helper..."
    
    local aur_helper=""
    for helper in yay paru trizen pikaur; do
        if command -v "$helper" >/dev/null 2>&1; then
            aur_helper="$helper"
            ok "Found AUR helper: $helper"
            break
        fi
    done
    
    if [[ -z "$aur_helper" ]]; then
        warn "No AUR helper found. Installing yay..."
        install_yay
        aur_helper="yay"
    fi
    
    info "Installing apexshot via $aur_helper..."
    $aur_helper -S --noconfirm apexshot
    
    ok "ApexShot installed from AUR"
}

install_yay() {
    step "Installing yay AUR helper"
    
    TMPDIR=$(mktemp -d -t yay-install.XXXXXX)
    
    prime_sudo
    
    run_spinner "Cloning yay repository..." \
        bash -c "cd '$TMPDIR' && git clone https://aur.archlinux.org/yay.git"
    
    run_spinner "Building yay..." \
        bash -c "cd '$TMPDIR/yay' && makepkg -si --noconfirm"
    
    rm -rf "$TMPDIR"
    ok "yay installed"
}

# --- Install from release ----------------------------------------------------

install_from_release() {
    step "Installing from GitHub release"
    
    # Arch Linux doesn't use .deb packages, so we need to check for
    # pre-built binaries or build from source
    warn "Pre-built binaries for Arch require manual download"
    info "Redirecting to source build..."
    
    install_from_source
}

# --- Install from source -----------------------------------------------------

install_from_source() {
    step "Building from source"
    
    TMPDIR=$(mktemp -d -t apexshot-build.XXXXXX)
    
    # Install build dependencies
    prime_sudo
    
    local deps=(
        rust
        cargo
        git
        cmake
        clang
        pkgconf
        gtk4
        libadwaita
        gtk4-layer-shell
        gst-libav
        gst-plugin-pipewire
        libpipewire
        tesseract
        qt5-base
        qt5-x11extras
        libxtst
        wl-clipboard
        xclip
        libnotify
        ffmpeg
        grim
    )
    
    info "Installing build dependencies..."
    run_spinner "Installing dependencies..." \
        bash -c "${SUDO} pacman -S --needed --noconfirm ${deps[*]}"
    
    ok "Dependencies installed"
    
    # Clone and build
    step "Cloning repository"
    run_spinner "Cloning..." \
        bash -c "git clone --depth 1 'https://github.com/${REPO}.git' '$TMPDIR/apexshot'"
    
    step "Building"
    run_spinner "Compiling (this may take several minutes)..." \
        bash -c "cd '$TMPDIR/apexshot' && cargo build --release"
    
    step "Installing"
    run_spinner "Installing to /usr/local/bin..." bash -c "${SUDO} cp '$TMPDIR/apexshot/target/release/apexshot' /usr/local/bin/"
    INSTALL_PATH="/usr/local/bin/apexshot"
    
    # Install bundled binaries if they exist
    if [[ -f "$TMPDIR/apexshot/target/release/apexshot-capture" ]]; then
        run_spinner "Installing capture overlay..." \
            bash -c "${SUDO} cp '$TMPDIR/apexshot/target/release/apexshot-capture' /usr/local/bin/"
    fi
    
    if [[ -f "$TMPDIR/apexshot/packaging/deb/apexshot-native-host" ]]; then
        run_spinner "Installing native host..." \
            bash -c "${SUDO} cp '$TMPDIR/apexshot/packaging/deb/apexshot-native-host' /usr/local/bin/"
    fi
    
    rm -rf "$TMPDIR"
    ok "Build complete"
}

# --- Setup -------------------------------------------------------------------

setup_browser_host() {
    step "Browser integration"
    
    if [[ $EUID -ne 0 ]]; then
        info "Skipping browser native host setup (requires root)."
        info "Run with sudo if you need browser integration."
        return
    fi
    
    # Create directories for Chrome/Chromium
    mkdir -p /etc/opt/chrome/NativeMessagingHosts
    mkdir -p /etc/chromium/NativeMessagingHosts
    
    ok "Browser native messaging host directories prepared"
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
    echo -e "  Binary:    ${BOLD}${INSTALL_PATH}${RESET}"
    echo -e "  Website:   ${DIM}https://apexshot.org/${RESET}"
    echo -e "  Issues:    ${DIM}https://github.com/${REPO}/issues${RESET}"
    echo -e "\n  ${BOLD}Quick start:${RESET}"
    echo -e "    apexshot capture screen    # Full-screen screenshot"
    echo -e "    apexshot capture area      # Area selection"
    echo -e "    apexshot record screen     # Start recording"
    echo -e "    apexshot settings          # Open settings"
    echo -e "\n  ${BOLD}AUR update:${RESET}"
    echo -e "    ${DIM}yay -Syu apexshot${RESET}"
    echo -e "${GREEN}${BOLD}═══════════════════════════════════════════════════════${RESET}\n"
}

# --- Main --------------------------------------------------------------------

main() {
    trap cleanup EXIT

    header
    check_prereqs
    select_install_method
    setup_browser_host
    summary
}

main "$@"
