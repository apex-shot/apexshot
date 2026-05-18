#!/bin/bash
# shellcheck shell=bash
set -euo pipefail

# ============================================================================
# ApexShot Installer for Arch Linux
# A stylish terminal UI for installing ApexShot on Arch-based systems.
# Usage: curl -fsSL https://raw.githubusercontent.com/apex-shot/apexshot/main/scripts/arch-install.sh | bash
# ============================================================================

REPO="apex-shot/apexshot"
RELEASES_URL="https://github.com/${REPO}/releases"
AUR_URL="https://aur.archlinux.org/packages/apexshot"
EXT_UUID="apexshot-gnome-integration@apexshot.github.io"
VERSION=""
TMPDIR=""
SUDO=""
INSTALL_PATH="/usr/bin/apexshot"
SCRIPT_NAME="arch-install"
TELEMETRY_CHANNEL="install"
TELEMETRY_URL="${APEXSHOT_TELEMETRY_URL:-https://apexshot.org/api/download-telemetry}"

SCRIPT_SOURCE="${BASH_SOURCE[0]:-}"
SCRIPT_DIR=""
if [[ -n "$SCRIPT_SOURCE" ]]; then
    SCRIPT_DIR="$(cd "$(dirname "$SCRIPT_SOURCE")" && pwd)"
fi

handoff_if_wrong_distro() {
    if ! command -v pacman >/dev/null 2>&1 && { command -v apt >/dev/null 2>&1 || command -v dpkg >/dev/null 2>&1; }; then
        echo "Ubuntu/Debian detected; switching to the Ubuntu/Debian installer."
        if [[ -n "$SCRIPT_DIR" && -f "${SCRIPT_DIR}/ubuntu-install.sh" ]]; then
            exec bash "${SCRIPT_DIR}/ubuntu-install.sh" "$@"
        fi
        exec bash -c "$(curl -fsSL https://raw.githubusercontent.com/${REPO}/main/scripts/ubuntu-install.sh)"
    fi
}

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

telemetry_enabled() {
    case "${APEXSHOT_TELEMETRY:-1}" in
        0|false|FALSE|no|NO|off|OFF) return 1 ;;
        *) return 0 ;;
    esac
}

json_escape() {
    local value=${1:-}
    value=${value//\\/\\\\}
    value=${value//\"/\\\"}
    value=${value//$'\n'/ }
    value=${value//$'\r'/ }
    printf '%s' "$value"
}

telemetry_distro() {
    if [[ -r /etc/os-release ]]; then
        (
            . /etc/os-release
            printf '%s' "${ID:-linux}"
            [[ -n "${VERSION_ID:-}" ]] && printf ':%s' "$VERSION_ID"
        )
    else
        printf 'linux'
    fi
}

current_desktop_id() {
    local desktop="${XDG_CURRENT_DESKTOP:-${DESKTOP_SESSION:-}}"
    printf '%s' "${desktop,,}"
}

arch_portal_backend_package() {
    local desktop
    desktop=$(current_desktop_id)

    if [[ -n "${HYPRLAND_INSTANCE_SIGNATURE:-}" ]] || [[ "$desktop" == *hyprland* ]]; then
        printf '%s' "xdg-desktop-portal-hyprland"
    elif [[ -n "${SWAYSOCK:-}" ]] || [[ "$desktop" == *sway* || "$desktop" == *river* || "$desktop" == *dwl* || "$desktop" == *wayfire* || "$desktop" == *labwc* || "$desktop" == *niri* ]]; then
        printf '%s' "xdg-desktop-portal-wlr"
    elif [[ "$desktop" == *kde* || "$desktop" == *plasma* ]]; then
        printf '%s' "xdg-desktop-portal-kde"
    elif [[ "$desktop" == *gnome* ]]; then
        printf '%s' "xdg-desktop-portal-gnome"
    else
        printf '%s' "xdg-desktop-portal-gtk"
    fi
}

send_download_telemetry() {
    telemetry_enabled || return 0

    local event=$1
    local asset_type=$2
    local status=${3:-}
    local size_bytes=${4:-0}
    local asset_name=${5:-}
    local distro
    distro=$(telemetry_distro)

    local payload
    payload=$(printf '{"event":"%s","script":"%s","distro":"%s","channel":"%s","version":"%s","asset_type":"%s","asset_name":"%s","status":"%s","size_bytes":%s}' \
        "$(json_escape "$event")" \
        "$(json_escape "$SCRIPT_NAME")" \
        "$(json_escape "$distro")" \
        "$(json_escape "$TELEMETRY_CHANNEL")" \
        "$(json_escape "${VERSION:-unknown}")" \
        "$(json_escape "$asset_type")" \
        "$(json_escape "$asset_name")" \
        "$(json_escape "$status")" \
        "$size_bytes")

    (curl -fsS -m 2 -H "Content-Type: application/json" -A "ApexShotDownloadTelemetry/${SCRIPT_NAME}" -d "$payload" "$TELEMETRY_URL" >/dev/null 2>&1 || true) &
}

download_file() {
    local url=$1
    local output=$2
    local asset_type=${3:-package}
    local asset_name=${output##*/}

    send_download_telemetry "download_started" "$asset_type" "started" 0 "$asset_name"

    if curl -fL --progress-bar -o "$output" "$url"; then
        local size_bytes=0
        size_bytes=$(stat -c%s "$output" 2>/dev/null || wc -c < "$output" 2>/dev/null || echo 0)
        send_download_telemetry "download_completed" "$asset_type" "success" "$size_bytes" "$asset_name"
    else
        local status=$?
        send_download_telemetry "download_failed" "$asset_type" "curl_${status}" 0 "$asset_name"
        return "$status"
    fi
}

latest_release_tag() {
    local effective tag
    effective=$(curl -fsSLI -o /dev/null -w '%{url_effective}' "${RELEASES_URL}/latest" || true)
    tag="${effective##*/}"

    if [[ -z "$tag" ]] || [[ "$tag" == "latest" ]]; then
        printf '%s' "${VERSION}"
    else
        printf '%s' "$tag"
    fi
}

resolve_latest_gnome_extension_url() {
    local extension_version
    extension_version=$(latest_release_tag)
    if [[ -z "$extension_version" ]]; then
        return 1
    fi

    local zip_path
    zip_path=$(curl -fsSL "${RELEASES_URL}/expanded_assets/${extension_version}" |
               grep -oE "/${REPO}/releases/download/${extension_version}/[^\"]*apexshot-gnome-integration\.zip" |
               head -n 1 || true)

    if [[ -z "$zip_path" ]]; then
        return 1
    fi

    printf 'https://github.com%s' "$zip_path"
}

is_gnome_session() {
    local desktop="${XDG_CURRENT_DESKTOP:-}:${XDG_SESSION_DESKTOP:-}:${DESKTOP_SESSION:-}"
    [[ -n "${GNOME_SETUP_DISPLAY:-}" ]] || [[ "${desktop,,}" == *gnome* ]]
}

should_skip_gnome_extension() {
    case "${APEXSHOT_SKIP_GNOME_EXTENSION:-}" in
        1|true|TRUE|yes|YES|on|ON) return 0 ;;
    esac

    ! is_gnome_session
}

prime_sudo() {
    if [[ -n "$SUDO" ]]; then
        $SUDO -v
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

    # Resolve sudo or fall back to root
    if [[ $EUID -eq 0 ]]; then
        SUDO=""
    elif command -v sudo >/dev/null 2>&1; then
        SUDO="sudo"
    else
        err "Root or sudo access is required to install packages."
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

# --- Installation method selection ------------------------------------------

select_install_method() {
    local choice="${APEXSHOT_ARCH_INSTALL_METHOD:-release}"

    while [[ $# -gt 0 ]]; do
        case "$1" in
            --method)
                shift
                choice="${1:-release}"
                ;;
            --method=*)
                choice="${1#--method=}"
                ;;
            --source)
                choice="source"
                ;;
            --aur)
                choice="aur"
                ;;
            --release|--github-release)
                choice="release"
                ;;
        esac
        shift || true
    done

    step "Selecting installation method"

    case $choice in
        1|release|github|github-release)
            ok "Using pre-built GitHub release package"
            install_from_release
            ;;
        2|source|build)
            ok "Building from source"
            install_from_source
            ;;
        3|aur)
            ok "Installing from AUR"
            install_from_aur
            ;;
        *)
            warn "Unknown method '${choice}' - using GitHub release package"
            install_from_release
            ;;
    esac
}

# --- Install from AUR --------------------------------------------------------

install_from_aur() {
    step "Installing from AUR"

    if pacman -Qq base-devel >/dev/null 2>&1; then
        ok "base-devel group found"
    else
        warn "base-devel group not found - required for AUR builds"
    fi
    
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

    fetch_version
    TMPDIR=$(mktemp -d -t apexshot-arch-install.XXXXXX)

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
    download_file "$pkg_url" "$pkg_file" "arch_package"
    ok "Package saved to ${pkg_file}"

    info "Stopping any running ApexShot daemon..."
    killall -9 apexshot 2>/dev/null || true

    prime_sudo
    info "Installing package with pacman:"
    ${SUDO} pacman -U --noconfirm --overwrite 'usr/share/apexshot/sounds/*.ogg' "${pkg_file}"
    INSTALL_PATH="/usr/bin/apexshot"

    ok "ApexShot installed from GitHub release"
}

# --- Install from source -----------------------------------------------------

install_from_source() {
    step "Building from source"
    
    TMPDIR=$(mktemp -d -t apexshot-build.XXXXXX)

    if pacman -Qq base-devel >/dev/null 2>&1; then
        ok "base-devel group found"
    else
        warn "base-devel group not found - installing build dependencies may not be enough without it"
    fi
    
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
        gstreamer
        gst-plugins-base
        gst-plugins-good
        gst-plugins-bad
        gst-libav
        gst-plugin-pipewire
        gst-plugin-libcamera
        libpipewire
        pipewire-libcamera
        libcamera
        libcamera-ipa
        tesseract
        tesseract-data-eng
        leptonica
        qt5-base
        qt5-x11extras
        qt5-wayland
        libxtst
        wl-clipboard
        xclip
        libnotify
        xdg-utils
        ffmpeg
        grim
        slurp
        xdg-desktop-portal
    )

    local portal_backend
    portal_backend=$(arch_portal_backend_package)
    deps+=("$portal_backend")
    
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

# --- GNOME Shell extension ---------------------------------------------------

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

install_gnome_extension() {
    step "Installing GNOME Shell extension"

    if should_skip_gnome_extension; then
        info "Skipping GNOME extension install because this does not look like a GNOME session."
        return
    fi

    local zip_url
    if ! zip_url=$(resolve_latest_gnome_extension_url); then
        warn "Latest GNOME extension zip not found in releases - package files were installed, but the user extension was not refreshed."
        return
    fi

    if [[ -z "${TMPDIR:-}" ]] || [[ ! -d "$TMPDIR" ]]; then
        TMPDIR=$(mktemp -d -t apexshot-arch-extension.XXXXXX)
    fi

    local zip_file="${TMPDIR}/apexshot-gnome-integration.zip"
    info "Downloading GNOME extension with progress:"
    download_file "$zip_url" "$zip_file" "gnome_extension"

    if ! command -v gnome-extensions >/dev/null 2>&1; then
        warn "gnome-extensions CLI not found - installing extension files directly."
        if install_gnome_extension_files "${zip_file}"; then
            ok "GNOME extension files installed"
            info "Log out and back in, then run: gnome-extensions enable ${EXT_UUID}"
        else
            warn "Could not install GNOME extension files automatically."
        fi
        return
    fi

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
            err "Could not install the GNOME extension."
            err "Try logging out and back in, then run this installer again."
            exit 1
        fi
    fi

    if run_gnome_extensions enable "${EXT_UUID}" >/dev/null 2>&1; then
        ok "GNOME extension installed and enabled"
    else
        warn "GNOME extension files were installed, but GNOME could not enable it in this session."
        info "Log out and back in, then run: gnome-extensions enable ${EXT_UUID}"
    fi
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
    echo -e "\n  ${BOLD}Update later with:${RESET}"
    echo -e "    ${DIM}curl -fsSL https://raw.githubusercontent.com/${REPO}/main/scripts/arch-update.sh | bash${RESET}"
    echo -e "${GREEN}${BOLD}═══════════════════════════════════════════════════════${RESET}\n"
}

# --- Main --------------------------------------------------------------------

main() {
    trap cleanup EXIT

    handoff_if_wrong_distro "$@"
    header
    check_prereqs
    select_install_method "$@"
    install_gnome_extension
    setup_browser_host
    summary
}

main "$@"
