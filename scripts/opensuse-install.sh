#!/bin/bash
# shellcheck shell=bash
set -euo pipefail

# ============================================================================
# ApexShot openSUSE Installer
# Builds ApexShot from source on openSUSE Tumbleweed/Leap.
# Usage: curl -fsSL https://raw.githubusercontent.com/apex-shot/apexshot/main/scripts/opensuse-install.sh | bash
# ============================================================================

REPO="apex-shot/apexshot"
TMPDIR=""
SUDO=""
INSTALL_ARGS=()

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
    echo -e "${DIM}      openSUSE source installer${RESET}\n"
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

run_spinner() {
    local msg=$1
    shift
    echo -e "  ${CYAN}…${RESET} ${msg}"
    "$@"
    ok "$msg"
}

cleanup() {
    if [[ -n "${TMPDIR:-}" && -d "$TMPDIR" ]]; then
        rm -rf "$TMPDIR"
    fi
}

check_prereqs() {
    step "Checking prerequisites"

    if ! command -v zypper >/dev/null 2>&1; then
        err "This installer is for openSUSE systems with zypper."
        err "Use scripts/install.sh on Ubuntu/Debian or Arch Linux."
        exit 1
    fi

    if [[ $EUID -eq 0 ]]; then
        SUDO=""
    elif command -v sudo >/dev/null 2>&1; then
        SUDO="sudo"
    else
        err "Root or sudo access is required to install dependencies."
        exit 1
    fi

    ok "zypper found"
}

prime_sudo() {
    if [[ -n "$SUDO" ]]; then
        $SUDO -v
    fi
}

portal_backend_package() {
    local desktop="${XDG_CURRENT_DESKTOP:-${XDG_SESSION_DESKTOP:-${DESKTOP_SESSION:-}}}"
    desktop="${desktop,,}"

    if [[ "$desktop" == *kde* || "$desktop" == *plasma* ]]; then
        printf '%s' "xdg-desktop-portal-kde"
    elif [[ "$desktop" == *gnome* ]]; then
        printf '%s' "xdg-desktop-portal-gnome"
    elif [[ -n "${SWAYSOCK:-}" || "$desktop" == *sway* || "$desktop" == *river* || "$desktop" == *wayfire* || "$desktop" == *niri* ]]; then
        printf '%s' "xdg-desktop-portal-wlr"
    else
        printf '%s' "xdg-desktop-portal-gtk"
    fi
}

install_dependencies() {
    step "Installing build and runtime dependencies"

    local deps=(
        build-essential
        cmake
        pkg-config
        libQt5Core-devel
        libQt5Widgets-devel
        libQt5DBus-devel
        libQt5Network-devel
        libqt5-qtx11extras-devel
        gstreamer-plugins-base-devel
        pipewire-devel
        tesseract-ocr-devel
        libgraphene-devel
        gtk4-devel
        gtk4-layer-shell-devel
        libadwaita-devel
        clang
        dbus-1-devel
        libXtst-devel
        curl-devel
        git
        rust
        cargo
        xdg-desktop-portal
        "$(portal_backend_package)"
        pipewire
        pipewire-pulseaudio
        wl-clipboard
        ffmpeg
    )

    prime_sudo
    run_spinner "Installing openSUSE packages" \
        bash -c "${SUDO} zypper --non-interactive install --needed ${deps[*]}"
}

build_and_install() {
    step "Building ApexShot"

    TMPDIR=$(mktemp -d -t apexshot-opensuse-install.XXXXXX)
    run_spinner "Cloning repository" \
        git clone --depth 1 "https://github.com/${REPO}.git" "$TMPDIR/apexshot"
    run_spinner "Compiling release binaries" \
        bash -c "cd '$TMPDIR/apexshot' && cargo build --release"

    step "Installing ApexShot"
    prime_sudo
    run_spinner "Installing binaries to /usr/local/bin" \
        bash -c "${SUDO} install -m 755 '$TMPDIR/apexshot/target/release/apexshot' /usr/local/bin/apexshot && ${SUDO} install -m 755 '$TMPDIR/apexshot/target/release/apexshot-capture' /usr/local/bin/apexshot-capture"

    if [[ -f "$TMPDIR/apexshot/packaging/deb/apexshot-native-host" ]]; then
        run_spinner "Installing browser native host helper" \
            bash -c "${SUDO} install -m 755 '$TMPDIR/apexshot/packaging/deb/apexshot-native-host' /usr/local/bin/apexshot-native-host"
    fi

    run_spinner "Installing desktop launchers and icons" \
        bash -c "${SUDO} install -Dm644 '$TMPDIR/apexshot/packaging/apexshot.desktop' /usr/local/share/applications/io.github.codegoddy.apexshot.desktop && ${SUDO} install -Dm644 '$TMPDIR/apexshot/packaging/apexshot-daemon.desktop' /usr/local/share/applications/io.github.codegoddy.apexshot.daemon.desktop && ${SUDO} install -Dm644 '$TMPDIR/apexshot/packaging/apexshot.svg' /usr/local/share/icons/hicolor/scalable/apps/io.github.codegoddy.apexshot.svg"

    run_spinner "Installing shared editor assets" \
        bash -c "${SUDO} mkdir -p /usr/local/share/apexshot/background-images /usr/local/share/apexshot/sounds && ${SUDO} cp '$TMPDIR/apexshot/src/capture/editor/background-images/'*.jpg /usr/local/share/apexshot/background-images/ && ${SUDO} cp '$TMPDIR/apexshot/assets/sounds/'*.ogg /usr/local/share/apexshot/sounds/"

    if command -v update-desktop-database >/dev/null 2>&1; then
        update-desktop-database /usr/local/share/applications 2>/dev/null || true
    fi

    if [[ $EUID -eq 0 && -n "${SUDO_USER:-}" && "${SUDO_USER}" != "root" ]]; then
        run_spinner "Installing user autostart and permissions" \
            sudo -u "${SUDO_USER}" env HOME="/home/${SUDO_USER}" /usr/local/bin/apexshot install --no-binary "${INSTALL_ARGS[@]}"
    else
        run_spinner "Installing user autostart and permissions" \
            /usr/local/bin/apexshot install --no-binary "${INSTALL_ARGS[@]}"
    fi
}

summary() {
    echo -e "\n${GREEN}${BOLD}═══════════════════════════════════════════════════════${RESET}"
    echo -e "${GREEN}${BOLD}  ApexShot is installed for openSUSE${RESET}\n"
    echo -e "  Binary:    ${BOLD}/usr/local/bin/apexshot${RESET}"
    echo -e "  Update:    ${DIM}curl -fsSL https://raw.githubusercontent.com/${REPO}/main/scripts/opensuse-update.sh | bash${RESET}"
    echo -e "  Remove:    ${DIM}apexshot uninstall --autostart-only && sudo apexshot uninstall${RESET}"
    echo -e "${GREEN}${BOLD}═══════════════════════════════════════════════════════${RESET}\n"
}

main() {
    trap cleanup EXIT

    while [[ $# -gt 0 ]]; do
        case "$1" in
            --no-autostart|--no-binary|--force)
                INSTALL_ARGS+=("$1")
                ;;
            *)
                err "Unknown option: $1"
                exit 1
                ;;
        esac
        shift
    done

    header
    check_prereqs
    install_dependencies
    build_and_install
    summary
}

main "$@"
