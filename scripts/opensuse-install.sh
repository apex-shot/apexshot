#!/bin/bash
# shellcheck shell=bash
set -euo pipefail

# ============================================================================
# ApexShot openSUSE Installer
# Installs ApexShot from published GitHub Release RPMs.
# Usage: curl -fsSL https://raw.githubusercontent.com/apex-shot/apexshot/main/scripts/opensuse-install.sh | bash
# ============================================================================

REPO="apex-shot/apexshot"
RELEASES_URL="https://github.com/${REPO}/releases"
VERSION=""
TMPDIR=""
SUDO=""
SCRIPT_NAME="opensuse-install"
TELEMETRY_CHANNEL="install"
TELEMETRY_URL="${APEXSHOT_TELEMETRY_URL:-https://apexshot.org/api/download-telemetry}"
INSTALL_ID=""
FORCE_REINSTALL=0
FROM_SOURCE=0

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
    echo -e "${DIM}      openSUSE RPM installer${RESET}\n"
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

ensure_install_id() {
    [[ -n "${INSTALL_ID:-}" ]] && return 0
    [[ -z "${HOME:-}" ]] && return 0

    local id_dir="${HOME}/.config/apexshot"
    local id_file="${id_dir}/install_id"

    if [[ -r "$id_file" ]]; then
        INSTALL_ID="$(cat "$id_file" 2>/dev/null || true)"
        [[ -n "$INSTALL_ID" ]] && return 0
    fi

    if command -v uuidgen >/dev/null 2>&1; then
        INSTALL_ID="$(uuidgen)"
    elif [[ -r /proc/sys/kernel/random/uuid ]]; then
        INSTALL_ID="$(cat /proc/sys/kernel/random/uuid)"
    else
        return 0
    fi

    mkdir -p "$id_dir" 2>/dev/null || true
    printf '%s' "$INSTALL_ID" > "$id_file" 2>/dev/null || true
}

send_download_telemetry() {
    telemetry_enabled || return 0
    [[ -z "${INSTALL_ID:-}" ]] && ensure_install_id

    local event=$1
    local asset_type=$2
    local status=${3:-}
    local size_bytes=${4:-0}
    local asset_name=${5:-}
    local distro
    distro=$(telemetry_distro)

    local install_id_json="null"
    [[ -n "$INSTALL_ID" ]] && install_id_json="\"$(json_escape "$INSTALL_ID")\""

    local payload
    payload=$(printf '{"event":"%s","script":"%s","distro":"%s","channel":"%s","version":"%s","asset_type":"%s","asset_name":"%s","status":"%s","size_bytes":%s,"install_id":%s}' \
        "$(json_escape "$event")" \
        "$(json_escape "$SCRIPT_NAME")" \
        "$(json_escape "$distro")" \
        "$(json_escape "$TELEMETRY_CHANNEL")" \
        "$(json_escape "${VERSION:-unknown}")" \
        "$(json_escape "$asset_type")" \
        "$(json_escape "$asset_name")" \
        "$(json_escape "$status")" \
        "$size_bytes" \
        "$install_id_json")

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

check_prereqs() {
    step "Checking prerequisites"

    if ! command -v zypper >/dev/null 2>&1; then
        err "This installer is for openSUSE systems with zypper."
        err "Use scripts/install.sh on Ubuntu/Debian, Arch, or Fedora."
        exit 1
    fi

    if [[ $EUID -eq 0 ]]; then
        SUDO=""
    elif command -v sudo >/dev/null 2>&1; then
        SUDO="sudo"
    else
        err "Root or sudo access is required to install ApexShot."
        exit 1
    fi

    if command -v curl >/dev/null 2>&1; then
        ok "curl found"
    else
        err "curl is required but not installed."
        err "Install it with: sudo zypper install curl"
        exit 1
    fi

    ok "zypper found"
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

install_runtime_dependencies() {
    step "Installing runtime dependencies"

    local deps=(
        curl
        ffmpeg
        gstreamer-plugins-base
        gstreamer-plugins-good
        gstreamer-plugins-bad
        gstreamer-plugin-pipewire
        pipewire
        pipewire-pulseaudio
        tesseract-ocr
        unzip
        wget
        wl-clipboard
        xdg-desktop-portal
        xdg-utils
        update-desktop-files
    )

    prime_sudo
    run_spinner "Installing openSUSE runtime packages" \
        bash -c "${SUDO} zypper --non-interactive install --needed ${deps[*]}"
}

resolve_rpm_url() {
    local rpm_path
    rpm_path=$(curl -fsSL "${RELEASES_URL}/expanded_assets/${VERSION}" |
               grep -oE "/${REPO}/releases/download/${VERSION}/[^\"]*\.x86_64\.rpm" |
               grep -v '\.src\.rpm$' |
               head -n 1 || true)

    if [[ -z "$rpm_path" ]]; then
        return 1
    fi

    printf 'https://github.com%s' "$rpm_path"
}

download_rpm() {
    step "Downloading ApexShot ${VERSION}"

    TMPDIR=$(mktemp -d -t apexshot-opensuse-install.XXXXXX)

    local rpm_url
    if ! rpm_url=$(resolve_rpm_url); then
        err "Could not find the openSUSE RPM download URL for ${VERSION}."
        err "If this release does not publish an openSUSE RPM yet, try a source install from a checked-out repo."
        exit 1
    fi

    local rpm_file="${TMPDIR}/apexshot-${VERSION#v}.x86_64.rpm"
    info "Downloading openSUSE RPM with progress:"
    download_file "$rpm_url" "$rpm_file" "opensuse_rpm"

    RPM_FILE="$rpm_file"
    ok "Package saved to ${rpm_file}"
}

install_rpm() {
    step "Installing ApexShot"

    if [[ -z "${RPM_FILE:-}" ]] || [[ ! -f "${RPM_FILE}" ]]; then
        err "RPM file is missing."
        exit 1
    fi

    prime_sudo
    if [[ $FORCE_REINSTALL -eq 1 ]]; then
        run_spinner "Reinstalling package..." \
            bash -c "${SUDO} zypper --non-interactive install --force '${RPM_FILE}'"
    else
        run_spinner "Installing package..." \
            bash -c "${SUDO} zypper --non-interactive install '${RPM_FILE}'"
    fi

    ok "ApexShot installed"
}

install_from_source() {
    step "Falling back to source installation"
    info "Published openSUSE RPM not available; using the source installer path is no longer embedded here."
    err "Use a checked-out repo copy if you need a source install."
    exit 1
}

installed_apexshot_version() {
    if ! command -v apexshot >/dev/null 2>&1 && [[ ! -x /usr/bin/apexshot ]]; then
        return 0
    fi
    local bin="apexshot"
    [[ -x /usr/bin/apexshot ]] && bin="/usr/bin/apexshot"
    "$bin" --version 2>/dev/null | awk '/^apexshot / { print $2; exit }'
}

run_as_desktop_user() {
    if [[ $EUID -ne 0 ]]; then
        "$@"
        return $?
    fi
    local user="${SUDO_USER:-}"
    if [[ -z "$user" || "$user" == "root" ]]; then
        return 1
    fi
    local target_uid runtime_dir bus_address
    target_uid=$(id -u "$user" 2>/dev/null || true)
    runtime_dir="${XDG_RUNTIME_DIR:-/run/user/${target_uid}}"
    bus_address="${DBUS_SESSION_BUS_ADDRESS:-unix:path=${runtime_dir}/bus}"
    if command -v sudo >/dev/null 2>&1; then
        sudo -u "$user" \
            env \
            DISPLAY="${DISPLAY:-}" \
            WAYLAND_DISPLAY="${WAYLAND_DISPLAY:-}" \
            XDG_SESSION_TYPE="${XDG_SESSION_TYPE:-}" \
            XDG_CURRENT_DESKTOP="${XDG_CURRENT_DESKTOP:-}" \
            XDG_RUNTIME_DIR="${runtime_dir}" \
            DBUS_SESSION_BUS_ADDRESS="${bus_address}" \
            "$@"
    elif command -v runuser >/dev/null 2>&1; then
        runuser -u "$user" -- \
            env \
            DISPLAY="${DISPLAY:-}" \
            WAYLAND_DISPLAY="${WAYLAND_DISPLAY:-}" \
            XDG_SESSION_TYPE="${XDG_SESSION_TYPE:-}" \
            XDG_CURRENT_DESKTOP="${XDG_CURRENT_DESKTOP:-}" \
            XDG_RUNTIME_DIR="${runtime_dir}" \
            DBUS_SESSION_BUS_ADDRESS="${bus_address}" \
            "$@"
    else
        return 1
    fi
}

post_install_launch() {
    step "Starting ApexShot"

    if [[ -z "${DISPLAY:-}" && -z "${WAYLAND_DISPLAY:-}" ]]; then
        info "No graphical session detected. Open ApexShot from the app menu after login."
        return 0
    fi

    local bin="/usr/bin/apexshot"
    if [[ ! -x "$bin" ]]; then
        if command -v apexshot >/dev/null 2>&1; then
            bin="$(command -v apexshot)"
        else
            warn "apexshot binary not found; open it from the app menu after install."
            return 0
        fi
    fi

    if ! run_as_desktop_user bash -c "
        '${bin}' daemon >/dev/null 2>&1 &
        sleep 0.6
        '${bin}' >/dev/null 2>&1 &
        true
    "; then
        if [[ $EUID -ne 0 ]]; then
            nohup "$bin" daemon >/dev/null 2>&1 &
            disown 2>/dev/null || true
            sleep 0.6
            nohup "$bin" >/dev/null 2>&1 &
            disown 2>/dev/null || true
            ok "Opened ApexShot (tray daemon + setup window)"
        else
            info "Could not launch GUI as a desktop user. Open ApexShot from the app menu."
        fi
        return 0
    fi

    ok "Opened ApexShot (tray daemon + setup window)"
    info "Look for the tray icon, or finish setup in the window that opened."
}

summary() {
    echo -e "\n${GREEN}${BOLD}═══════════════════════════════════════════════════════${RESET}"
    echo -e "${GREEN}${BOLD}  ApexShot is installed for openSUSE${RESET}\n"
    echo -e "  Version:   ${BOLD}${VERSION}${RESET}"
    echo -e "  Binary:    ${BOLD}/usr/bin/apexshot${RESET}"
    echo -e "\n  ${BOLD}How to use:${RESET}"
    echo -e "    Open ${BOLD}ApexShot${RESET} from the app menu (Settings / first-run setup)"
    echo -e "    Tray icon + hotkeys handle day-to-day capture"
    echo -e "  Update:    ${DIM}curl -fsSL https://raw.githubusercontent.com/${REPO}/main/scripts/opensuse-update.sh | bash${RESET}"
    echo -e "  Remove:    ${DIM}sudo zypper remove apexshot${RESET}"
    echo -e "  ${DIM}Re-run this installer with --force to re-download the package.${RESET}"
    echo -e "${GREEN}${BOLD}═══════════════════════════════════════════════════════${RESET}\n"
}

main() {
    trap cleanup EXIT

    while [[ $# -gt 0 ]]; do
        case "$1" in
            --force)
                FORCE_REINSTALL=1
                ;;
            --from-source)
                FROM_SOURCE=1
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
    fetch_version

    local latest_ver="${VERSION#v}"
    local current_ver
    current_ver="$(installed_apexshot_version || true)"
    if [[ $FORCE_REINSTALL -eq 0 && $FROM_SOURCE -eq 0 && -n "$current_ver" && "$current_ver" == "$latest_ver" ]]; then
        ok "ApexShot ${BOLD}${current_ver}${RESET} is already installed"
        info "Skipping re-download. Use --force to reinstall the package."
        post_install_launch
        summary
        return 0
    fi

    install_runtime_dependencies

    if [[ $FROM_SOURCE -eq 1 ]]; then
        install_from_source
    fi

    download_rpm
    install_rpm
    post_install_launch
    summary
}

main "$@"
