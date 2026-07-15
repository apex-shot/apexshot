#!/bin/bash
# shellcheck shell=bash
set -euo pipefail

# ============================================================================
# ApexShot Ubuntu/Debian/Pop!_OS Installer
# A stylish terminal UI for installing ApexShot and its dependencies.
# Usage: curl -fsSL https://raw.githubusercontent.com/apex-shot/apexshot/main/scripts/ubuntu-install.sh | bash
# ============================================================================

REPO="apex-shot/apexshot"
RELEASES_URL="https://github.com/${REPO}/releases"
EXT_UUID="apexshot-gnome-integration@apexshot.github.io"
VERSION=""
TMPDIR=""
SUDO=""
SCRIPT_NAME="ubuntu-install"
TELEMETRY_CHANNEL="install"
TELEMETRY_URL="${APEXSHOT_TELEMETRY_URL:-https://apexshot.org/api/download-telemetry}"
INSTALL_ID=""

SCRIPT_SOURCE="${BASH_SOURCE[0]:-}"
SCRIPT_DIR=""
if [[ -n "$SCRIPT_SOURCE" ]]; then
    SCRIPT_DIR="$(cd "$(dirname "$SCRIPT_SOURCE")" && pwd)"
fi

handoff_if_wrong_distro() {
    if command -v pacman >/dev/null 2>&1 && ! command -v apt >/dev/null 2>&1; then
        echo "Arch Linux detected; switching to the Arch installer."
        if [[ -n "$SCRIPT_DIR" && -f "${SCRIPT_DIR}/arch-install.sh" ]]; then
            exec bash "${SCRIPT_DIR}/arch-install.sh" "$@"
        fi
        exec bash -c "$(curl -fsSL https://raw.githubusercontent.com/${REPO}/main/scripts/arch-install.sh)"
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
    wait "$pid"
    local rc=$?
    if [[ $rc -eq 0 ]]; then
        printf "\r  ${GREEN}✔${RESET} %s\n" "$msg"
    else
        printf "\r  ${RED}✖${RESET} %s\n" "$msg"
    fi
    return $rc
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

current_desktop_id() {
    local desktop="${XDG_CURRENT_DESKTOP:-}:${XDG_SESSION_DESKTOP:-}:${DESKTOP_SESSION:-}"
    printf '%s' "${desktop,,}"
}

is_gnome_session() {
    # GNOME_DESKTOP_SESSION_ID is the canonical signal set by gnome-session
    # itself. XDG_CURRENT_DESKTOP / GDMSESSION / GNOME_SETUP_DISPLAY are
    # fallbacks for edge cases (runner wrappers, nested sessions, etc.).
    [[ -n "${GNOME_DESKTOP_SESSION_ID:-}" ]] && return 0
    [[ -n "${GNOME_SETUP_DISPLAY:-}" ]] && return 0

    local desktop
    desktop=$(current_desktop_id)
    [[ "$desktop" == *gnome* ]]
}

portal_backend_packages() {
    local desktop packages
    desktop=$(current_desktop_id)

    if [[ -n "${HYPRLAND_INSTANCE_SIGNATURE:-}" ]] || [[ "$desktop" == *hyprland* ]]; then
        packages="xdg-desktop-portal xdg-desktop-portal-hyprland"
    elif [[ -n "${SWAYSOCK:-}" ]] || [[ "$desktop" == *sway* || "$desktop" == *river* || "$desktop" == *dwl* || "$desktop" == *wayfire* || "$desktop" == *labwc* || "$desktop" == *niri* ]]; then
        packages="xdg-desktop-portal xdg-desktop-portal-wlr"
    elif [[ "$desktop" == *kde* || "$desktop" == *plasma* ]]; then
        packages="xdg-desktop-portal xdg-desktop-portal-kde"
    elif is_gnome_session; then
        packages="xdg-desktop-portal xdg-desktop-portal-gnome"
    elif [[ "$desktop" == *cosmic* ]]; then
        packages="xdg-desktop-portal xdg-desktop-portal-cosmic"
    else
        packages="xdg-desktop-portal xdg-desktop-portal-gtk"
    fi

    printf '%s' "$packages"
}

should_skip_gnome_extension() {
    case "${APEXSHOT_SKIP_GNOME_EXTENSION:-}" in
        1|true|TRUE|yes|YES|on|ON) return 0 ;;
    esac

    ! is_gnome_session
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

    if command -v apt >/dev/null 2>&1; then
        ok "apt package manager found"
    else
        err "This installer currently supports Debian/Ubuntu/Pop!_OS (apt)."
        err "For Arch Linux, use: scripts/arch-install.sh"
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

# --- Install system dependencies ---------------------------------------------

# True when the dynamic linker can already resolve libgtk4-layer-shell.so.0
# (either the distro package or a previous from-source install).
has_gtk4_layer_shell_lib() {
    if dpkg -s libgtk4-layer-shell0 >/dev/null 2>&1; then
        return 0
    fi
    if command -v ldconfig >/dev/null 2>&1; then
        if ldconfig -p 2>/dev/null | grep -q 'libgtk4-layer-shell\.so\.0'; then
            return 0
        fi
    fi
    # Common install locations used by distro packages and our source fallback.
    local candidate
    for candidate in \
        /usr/lib/x86_64-linux-gnu/libgtk4-layer-shell.so.0 \
        /usr/lib/aarch64-linux-gnu/libgtk4-layer-shell.so.0 \
        /usr/lib/libgtk4-layer-shell.so.0 \
        /usr/local/lib/libgtk4-layer-shell.so.0 \
        /usr/local/lib/x86_64-linux-gnu/libgtk4-layer-shell.so.0
    do
        if [[ -e "$candidate" ]]; then
            return 0
        fi
    done
    return 1
}

# Ubuntu 24.04 (noble) and several Mint/older bases do not ship
# libgtk4-layer-shell0. The prebuilt binary still needs the shared library
# (NEEDED: libgtk4-layer-shell.so.0), so fall back to the same from-source
# install path used by CI. Distros that already have the package keep the
# apt path unchanged.
install_gtk4_layer_shell_from_source() {
    step "Installing libgtk4-layer-shell from source"

    if has_gtk4_layer_shell_lib; then
        ok "libgtk4-layer-shell already available on this system"
        return 0
    fi

    info "libgtk4-layer-shell0 is not in your apt repositories (common on Ubuntu 24.04 / Mint)."
    info "Building a compatible runtime library from source — this may take a minute..."

    # Build tools only; not required at ApexShot runtime after install.
    local build_deps=(
        build-essential git meson ninja-build pkg-config
        libgtk-4-dev libwayland-dev
        gobject-introspection libgirepository1.0-dev
        valac
    )
    local build_missing=()
    local pkg
    for pkg in "${build_deps[@]}"; do
        if dpkg -s "$pkg" >/dev/null 2>&1; then
            continue
        fi
        if apt-cache show "$pkg" >/dev/null 2>&1; then
            build_missing+=("$pkg")
        fi
    done

    if [[ ${#build_missing[@]} -gt 0 ]]; then
        info "Installing build tools: ${build_missing[*]}"
        if ! run_spinner "Installing build tools..." bash -c "${SUDO} apt-get install -y -qq ${build_missing[*]}"; then
            err "Failed to install build tools needed for gtk4-layer-shell."
            err "Install them manually and re-run, or use Ubuntu 25.10+ where libgtk4-layer-shell0 is packaged."
            exit 1
        fi
    fi

    # Pin a stable release that matches the Ubuntu 25.10 package series so
    # runtime behavior stays close to the official .deb CI build target.
    local layer_shell_tag="${APEXSHOT_GTK4_LAYER_SHELL_TAG:-v1.0.4}"
    local src_dir
    src_dir=$(mktemp -d -t apexshot-gtk4-layer-shell.XXXXXX)

    cleanup_layer_shell_src() {
        rm -rf "${src_dir}"
    }

    if ! run_spinner "Cloning gtk4-layer-shell ${layer_shell_tag}..." \
        bash -c "git clone --depth 1 --branch '${layer_shell_tag}' https://github.com/wmww/gtk4-layer-shell.git '${src_dir}/src'"; then
        # Tag may not exist on older git mirrors; fall back to default branch.
        warn "Tagged clone failed; falling back to latest default branch."
        if ! run_spinner "Cloning gtk4-layer-shell..." \
            bash -c "git clone --depth 1 https://github.com/wmww/gtk4-layer-shell.git '${src_dir}/src'"; then
            cleanup_layer_shell_src
            err "Could not clone gtk4-layer-shell. Check your network and try again."
            exit 1
        fi
    fi

    # Same prefix as the release CI job so the soname lands on the default
    # linker path without extra ld.so.conf entries.
    if ! run_spinner "Building gtk4-layer-shell..." \
        bash -c "cd '${src_dir}/src' && meson setup build --prefix=/usr && ninja -C build"; then
        cleanup_layer_shell_src
        err "Failed to build gtk4-layer-shell from source."
        exit 1
    fi

    if ! run_spinner "Installing gtk4-layer-shell..." \
        bash -c "cd '${src_dir}/src' && ${SUDO} ninja -C build install && ${SUDO} ldconfig"; then
        cleanup_layer_shell_src
        err "Failed to install gtk4-layer-shell."
        exit 1
    fi

    cleanup_layer_shell_src

    if ! has_gtk4_layer_shell_lib; then
        err "gtk4-layer-shell installed, but libgtk4-layer-shell.so.0 is still not visible to the linker."
        err "Please open an issue with your distro/version: https://github.com/${REPO}/issues"
        exit 1
    fi

    ok "libgtk4-layer-shell installed from source"
}

warn_if_x11_session() {
    # ApexShot has an X11 backend, but Wayland (especially GNOME) is the
    # personally tested path. X11 remains experimental — do not block install.
    if [[ -n "${WAYLAND_DISPLAY:-}" ]]; then
        return 0
    fi
    if [[ -z "${DISPLAY:-}" ]]; then
        return 0
    fi
    warn "X11 session detected. ApexShot supports X11, but it is experimental."
    info "Primary testing is on GNOME Wayland (Ubuntu/Arch) and Hyprland."
    info "If something breaks on X11, please report it: https://github.com/${REPO}/issues"
}

install_deps() {
    step "Installing system dependencies"

    info "This may take a few minutes..."

    # This installer installs a prebuilt .deb, so only runtime packages belong
    # here. Do not install -dev/build packages unless we hit the
    # libgtk4-layer-shell from-source fallback below.
    #
    # libgtk4-layer-shell0 is preferred from apt when present (Ubuntu 25.10+,
    # Debian with the package). On Ubuntu 24.04 / some Mint bases it is
    # missing from apt — those systems use install_gtk4_layer_shell_from_source.
    local deps=(
        libx11-6 libxext6 libxtst6
        libqt5widgets5 libqt5dbus5 libqt5network5 libqt5x11extras5
        gstreamer1.0-plugins-base gstreamer1.0-plugins-good
        gstreamer1.0-plugins-bad gstreamer1.0-libav gstreamer1.0-pipewire
        tesseract-ocr tesseract-ocr-eng
        libgtk-4-1 libadwaita-1-0
        wl-clipboard xclip
        xdg-utils libnotify-bin ffmpeg unzip curl wget
        pipewire pipewire-pulse
    )

    local need_layer_shell_from_source=0
    if apt-cache show libgtk4-layer-shell0 >/dev/null 2>&1; then
        deps+=(libgtk4-layer-shell0)
    else
        need_layer_shell_from_source=1
    fi

    local portal_pkg
    for portal_pkg in $(portal_backend_packages); do
        deps+=("$portal_pkg")
    done

    prime_sudo

    # Update apt before checking availability so derivatives with stale package
    # lists do not produce false "Unable to locate package" failures.
    run_spinner "Updating package lists..." bash -c "${SUDO} apt-get update -qq"

    # Re-evaluate after apt-get update in case a stale cache hid the package.
    if [[ $need_layer_shell_from_source -eq 1 ]] && apt-cache show libgtk4-layer-shell0 >/dev/null 2>&1; then
        deps+=(libgtk4-layer-shell0)
        need_layer_shell_from_source=0
    fi

    local missing=()
    local unavailable=()
    local pkg
    for pkg in "${deps[@]}"; do
        if dpkg -s "$pkg" >/dev/null 2>&1; then
            continue
        fi
        if apt-cache show "$pkg" >/dev/null 2>&1; then
            missing+=("$pkg")
        else
            unavailable+=("$pkg")
        fi
    done

    if [[ ${#unavailable[@]} -gt 0 ]]; then
        err "Your apt repositories do not provide required ApexShot runtime package(s): ${unavailable[*]}"
        err "Please open an issue with your distro/version, or use a newer Ubuntu/Debian base that provides these packages."
        exit 1
    fi

    if [[ ${#missing[@]} -gt 0 ]]; then
        info "Missing packages: ${missing[*]}"
        run_spinner "Installing missing packages..." bash -c "${SUDO} apt-get install -y -qq ${missing[*]}"
        ok "Dependencies installed"
    else
        ok "All apt dependencies already satisfied"
    fi

    if [[ $need_layer_shell_from_source -eq 1 ]]; then
        install_gtk4_layer_shell_from_source
    elif ! has_gtk4_layer_shell_lib; then
        # Package was supposed to be available but the library is still missing
        # (broken local install, partial purge, etc.).
        warn "libgtk4-layer-shell0 was expected from apt but the shared library is not visible."
        install_gtk4_layer_shell_from_source
    fi

    warn_if_x11_session
}

# --- Download .deb -----------------------------------------------------------

download_deb() {
    step "Downloading ApexShot ${VERSION}"

    TMPDIR=$(mktemp -d -t apexshot-install.XXXXXX)
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
    download_file "$deb_url" "$deb_file" "deb"

    ok "Package saved to ${deb_file}"
}

# --- Install .deb ------------------------------------------------------------

install_deb() {
    step "Installing ApexShot"

    local deb_file="${TMPDIR}/apexshot_${VERSION}_amd64.deb"
    prime_sudo
    if ! run_spinner "Installing package..." bash -c "${SUDO} dpkg -i '${deb_file}' && ${SUDO} apt install -f -y -qq"; then
        err "Package installation failed."
        err "Try manual install: ${SUDO} dpkg -i '${deb_file}'"
        err "Then resolve deps: ${SUDO} apt install -f"
        exit 1
    fi

    ok "ApexShot installed"
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

remove_stale_home_binary() {
    local name=$1
    local candidate="$2"
    [[ -x "$candidate" ]] || return 0
    if ! "$candidate" --version 2>/dev/null | grep -Eq '^apexshot [0-9]+\.[0-9]+\.[0-9]+'; then
        return 0
    fi
    local backup="${candidate}.pre-install.$(date +%Y%m%d%H%M%S)"
    mv "$candidate" "$backup"
    ok "Moved stale ${candidate} to ${backup}"
}

cleanup_shadowing_home_binaries() {
    remove_stale_home_binary apexshot "${HOME}/.cargo/bin/apexshot"
    remove_stale_home_binary apexshot "${HOME}/.local/bin/apexshot"
    hash -r 2>/dev/null || true
}

capture_backend_summary() {
    if is_gnome_session; then
        printf '%s' "GNOME Shell D-Bus API for screenshots; ScreenCast portal + PipeWire for recording."
    else
        printf '%s' "wlr-screencopy / Screenshot portal for screenshots; ScreenCast portal + PipeWire/wf-recorder for recording."
    fi
}

summary() {
    echo -e "\n${GREEN}${BOLD}═══════════════════════════════════════════════════════${RESET}"
    echo -e "${GREEN}${BOLD}  ApexShot is ready!${RESET}\n"
    echo -e "  Version:   ${BOLD}${VERSION}${RESET}"
    echo -e "  Binary:    ${BOLD}/usr/bin/apexshot${RESET}"
    echo -e "  Capture:   ${BOLD}$(capture_backend_summary)${RESET}"
    echo -e "  Website:   ${DIM}https://apexshot.org/${RESET}"
    echo -e "  Issues:    ${DIM}https://github.com/${REPO}/issues${RESET}"
    echo -e "\n  ${BOLD}Quick start:${RESET}"
    echo -e "    apexshot capture screen    # Full-screen screenshot"
    echo -e "    apexshot capture area      # Area selection"
    echo -e "    apexshot record screen     # Start recording"
    echo -e "    apexshot settings          # Open settings"
    echo -e "\n  ${BOLD}Update later with:${RESET}"
    echo -e "    ${DIM}curl -fsSL https://raw.githubusercontent.com/${REPO}/main/scripts/ubuntu-update.sh | bash${RESET}"
    echo -e "${GREEN}${BOLD}═══════════════════════════════════════════════════════${RESET}\n"
}

# --- Main --------------------------------------------------------------------

main() {
    trap cleanup EXIT

    handoff_if_wrong_distro "$@"
    header
    check_prereqs
    fetch_version
    install_deps
    download_deb
    install_deb
    install_gnome_extension
    setup_browser_host
    cleanup_shadowing_home_binaries
    summary
}

main "$@"
