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

summary() {
    echo -e "\n${GREEN}${BOLD}═══════════════════════════════════════════════════════${RESET}"
    echo -e "${GREEN}${BOLD}  ApexShot is installed for openSUSE${RESET}\n"
    echo -e "  Version:   ${BOLD}${VERSION}${RESET}"
    echo -e "  Binary:    ${BOLD}/usr/bin/apexshot${RESET}"
    echo -e "  Update:    ${DIM}curl -fsSL https://raw.githubusercontent.com/${REPO}/main/scripts/opensuse-update.sh | bash${RESET}"
    echo -e "  Remove:    ${DIM}sudo zypper remove apexshot${RESET}"
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
    install_runtime_dependencies

    if [[ $FROM_SOURCE -eq 1 ]]; then
        install_from_source
    fi

    download_rpm
    install_rpm
    summary
}

main "$@"
