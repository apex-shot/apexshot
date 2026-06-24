#!/bin/bash
# shellcheck shell=bash
set -euo pipefail

# ============================================================================
# ApexShot Fedora Installer
# Installs ApexShot from published GitHub Release RPMs.
# Usage: curl -fsSL https://raw.githubusercontent.com/apex-shot/apexshot/main/scripts/fedora-install.sh | bash
# ============================================================================

REPO="apex-shot/apexshot"
RELEASES_URL="https://github.com/${REPO}/releases"
VERSION=""
TMPDIR=""
SUDO=""
SCRIPT_NAME="fedora-install"
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
    echo -e "${DIM}      Fedora RPM installer${RESET}\n"
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

    if ! command -v dnf >/dev/null 2>&1; then
        err "This installer is for Fedora systems with dnf."
        err "Use scripts/install.sh on Ubuntu/Debian, Arch, or openSUSE."
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
        err "Install it with: sudo dnf install curl"
        exit 1
    fi

    ok "dnf found"
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
        ffmpeg-free
        gstreamer1-plugins-base
        gstreamer1-plugins-good
        gstreamer1-plugins-bad-free
        gstreamer1-plugin-libav
        pipewire
        pipewire-pulseaudio
        tesseract
        unzip
        wget
        wl-clipboard
        xdg-desktop-portal
        xdg-utils
        desktop-file-utils
        hicolor-icon-theme
    )

    prime_sudo
    run_spinner "Installing Fedora runtime packages" \
        bash -c "${SUDO} dnf install -y ${deps[*]}"
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

    TMPDIR=$(mktemp -d -t apexshot-fedora-install.XXXXXX)

    local rpm_url
    if ! rpm_url=$(resolve_rpm_url); then
        err "Could not find the Fedora RPM download URL for ${VERSION}."
        err "If this release is still publishing, try again in a few minutes."
        err "Or use --from-source as a fallback."
        exit 1
    fi

    local rpm_file="${TMPDIR}/apexshot-${VERSION#v}.x86_64.rpm"
    info "Downloading Fedora RPM with progress:"
    download_file "$rpm_url" "$rpm_file" "fedora_rpm"

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
            bash -c "${SUDO} dnf reinstall -y '${RPM_FILE}' || ${SUDO} dnf install -y '${RPM_FILE}'"
    else
        run_spinner "Installing package..." \
            bash -c "${SUDO} dnf install -y '${RPM_FILE}'"
    fi

    if command -v restorecon >/dev/null 2>&1; then
        run_spinner "Refreshing SELinux labels" \
            bash -c "${SUDO} restorecon -v /usr/bin/apexshot /usr/bin/apexshot-capture 2>/dev/null || true"
    fi

    ok "ApexShot installed"
}

install_from_source() {
    step "Falling back to source installation"
    info "Published Fedora RPM not available; using the source installer path."
    local script_url="https://raw.githubusercontent.com/${REPO}/main/scripts/fedora-install.sh"
    err "Source fallback is not embedded in this release-first installer anymore."
    err "Use a checked-out repo copy if you need source installs."
    err "Expected script: ${script_url}"
    exit 1
}

summary() {
    echo -e "\n${GREEN}${BOLD}═══════════════════════════════════════════════════════${RESET}"
    echo -e "${GREEN}${BOLD}  ApexShot is installed for Fedora${RESET}\n"
    echo -e "  Version:   ${BOLD}${VERSION}${RESET}"
    echo -e "  Binary:    ${BOLD}/usr/bin/apexshot${RESET}"
    echo -e "  Update:    ${DIM}curl -fsSL https://raw.githubusercontent.com/${REPO}/main/scripts/fedora-update.sh | bash${RESET}"
    echo -e "  Remove:    ${DIM}sudo dnf remove apexshot${RESET}"
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
