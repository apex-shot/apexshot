#!/bin/bash
# shellcheck shell=bash
set -euo pipefail

# ============================================================================
# ApexShot Fedora local reinstall (fast test loop)
#
# Removes any installed system ApexShot, rebuilds from the current working
# tree with local `cargo build --release` (reuses target/ cache), then
# installs into the real system package paths (/usr/bin/apexshot, etc.).
#
# This installs your local changes (including uncommitted edits) without a
# full rpmbuild from-scratch compile.
#
# Flow:
#   1. stop running apexshot
#   2. remove installed apexshot package / previous system install
#   3. cargo build --release (local, incremental)
#   4. install binaries + assets to package paths
#
# Usage:
#   ./scripts/fedora-reinstall.sh              # remove + build + install
#   ./scripts/fedora-reinstall.sh --no-build   # remove + install last build
#   ./scripts/fedora-reinstall.sh --start      # also start the daemon
#   ./scripts/fedora-reinstall.sh --no-stop    # do not stop processes first
#   ./scripts/fedora-reinstall.sh --no-remove  # skip removal (overwrite)
#   ./scripts/fedora-reinstall.sh --rpm        # slow path: full rpmbuild + dnf
# ============================================================================

SCRIPT_SOURCE="${BASH_SOURCE[0]:-}"
SCRIPT_DIR="$(cd "$(dirname "$SCRIPT_SOURCE")" && pwd)"
REPO_DIR="$(cd "${SCRIPT_DIR}/.." && pwd)"
SPEC_SRC="${REPO_DIR}/packaging/fedora/apexshot.spec"
RPM_TOPDIR="${RPM_TOPDIR:-${REPO_DIR}/target/fedora-rpmbuild}"

NO_BUILD=0
START_DAEMON=0
NO_STOP=0
NO_REMOVE=0
USE_RPM=0
SUDO=""

BOLD="\033[1m"
DIM="\033[2m"
RESET="\033[0m"
RED="\033[31m"
GREEN="\033[32m"
YELLOW="\033[33m"
BLUE="\033[34m"
CYAN="\033[36m"

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

usage() {
    cat <<'EOF'
Usage: fedora-reinstall.sh [options]

Remove system ApexShot, rebuild from the current tree, and install into
the real package paths (/usr/bin/apexshot — not apexshot-dev).

Default (fast) flow: stop → remove → cargo build --release → install files

Options:
  --no-build    Skip compile; install existing target/release binaries
  --start       Start `apexshot daemon` after install
  --no-stop     Do not stop running ApexShot processes first
  --no-remove   Do not remove the installed package first
  --rpm         Slow path: full rpmbuild + dnf install (clean package)
  -h, --help    Show this help

Environment:
  RPM_TOPDIR    Override rpmbuild topdir when using --rpm
                (default: <repo>/target/fedora-rpmbuild)
EOF
}

parse_args() {
    while [[ $# -gt 0 ]]; do
        case "$1" in
            --no-build)
                NO_BUILD=1
                ;;
            --start)
                START_DAEMON=1
                ;;
            --no-stop)
                NO_STOP=1
                ;;
            --no-remove)
                NO_REMOVE=1
                ;;
            --rpm)
                USE_RPM=1
                ;;
            -h|--help)
                usage
                exit 0
                ;;
            *)
                err "Unknown option: $1"
                usage
                exit 1
                ;;
        esac
        shift
    done
}

require_fedora() {
    if ! command -v dnf >/dev/null 2>&1; then
        err "This script is for Fedora systems with dnf."
        exit 1
    fi
}

setup_sudo() {
    if [[ $EUID -eq 0 ]]; then
        SUDO=""
    elif command -v sudo >/dev/null 2>&1; then
        SUDO="sudo"
    else
        err "Root or sudo access is required to install the package."
        exit 1
    fi
}

prime_sudo() {
    if [[ -n "$SUDO" ]]; then
        $SUDO -v
    fi
}

package_version() {
    local version
    version="$(sed -n 's/^version = "\(.*\)"/\1/p' "${REPO_DIR}/Cargo.toml" | head -n 1)"
    version="${version#v}"
    if [[ -z "$version" ]]; then
        err "Could not read package version from Cargo.toml"
        exit 1
    fi
    printf '%s' "$version"
}

stop_running_app() {
    step "Stopping running ApexShot processes"

    if ! pgrep -x apexshot >/dev/null 2>&1 && ! pgrep -x apexshot-capture >/dev/null 2>&1; then
        ok "No running ApexShot processes"
        return 0
    fi

    info "Stopping: apexshot / apexshot-capture"
    pkill -x apexshot 2>/dev/null || true
    pkill -x apexshot-capture 2>/dev/null || true
    sleep 0.4
    if pgrep -x apexshot >/dev/null 2>&1 || pgrep -x apexshot-capture >/dev/null 2>&1; then
        pkill -9 -x apexshot 2>/dev/null || true
        pkill -9 -x apexshot-capture 2>/dev/null || true
    fi
    ok "ApexShot processes stopped"
}

# Paths installed by both the Fedora RPM and this script's fast path.
system_install_paths() {
    cat <<'EOF'
/usr/bin/apexshot
/usr/bin/apexshot-capture
/usr/bin/apexshot-native-host
/usr/share/applications/io.github.codegoddy.apexshot.desktop
/etc/xdg/autostart/apexshot.desktop
/usr/share/icons/hicolor/scalable/apps/apexshot.svg
/usr/share/icons/hicolor/scalable/apps/io.github.codegoddy.apexshot.svg
/usr/share/pixmaps/apexshot.svg
/etc/opt/chrome/NativeMessagingHosts/io.github.codegoddy.apexshot.json
/etc/chromium/NativeMessagingHosts/io.github.codegoddy.apexshot.json
/usr/share/gnome-shell/extensions/apexshot-gnome-integration@apexshot.github.io
/usr/share/apexshot
EOF
}

remove_system_files() {
    local path
    while IFS= read -r path; do
        [[ -z "$path" ]] && continue
        if [[ -e "$path" || -L "$path" ]]; then
            $SUDO rm -rf "$path"
        fi
    done < <(system_install_paths)
}

remove_installed_package() {
    step "Removing installed system package / previous install"
    prime_sudo

    local removed_any=0
    local pkg
    for pkg in apexshot apexshot-debuginfo apexshot-debugsource; do
        if rpm -q "$pkg" >/dev/null 2>&1; then
            local ver
            ver="$(rpm -q --qf '%{NAME}-%{VERSION}-%{RELEASE}' "$pkg" 2>/dev/null || echo "$pkg")"
            info "Removing ${ver}"
            $SUDO dnf remove -y "$pkg"
            removed_any=1
        fi
    done

    # Always clear package-layout files so a previous fast install is gone too.
    local leftover=0
    local path
    while IFS= read -r path; do
        [[ -z "$path" ]] && continue
        if [[ -e "$path" || -L "$path" ]]; then
            leftover=1
            break
        fi
    done < <(system_install_paths)

    if [[ $leftover -eq 1 ]]; then
        info "Clearing system install paths"
        remove_system_files
        removed_any=1
    fi

    if [[ $removed_any -eq 0 ]]; then
        ok "No ApexShot package was installed"
    else
        ok "Installed ApexShot removed from the system"
    fi
}

ensure_build_tools() {
    step "Checking build tools"

    local missing=()
    command -v cargo >/dev/null 2>&1 || missing+=(cargo)
    command -v cmake >/dev/null 2>&1 || missing+=(cmake)
    if [[ $USE_RPM -eq 1 ]]; then
        command -v rpmbuild >/dev/null 2>&1 || missing+=(rpm-build)
    fi

    if [[ ${#missing[@]} -gt 0 ]]; then
        warn "Missing packages: ${missing[*]}"
        info "Installing build tools with dnf..."
        prime_sudo
        # shellcheck disable=SC2086
        $SUDO dnf install -y ${missing[*]}
    fi

    ok "Build tools available"
}

build_local() {
    local version
    version="$(package_version)"

    step "Building ApexShot ${version} (local cargo release)"
    info "Reuses ${REPO_DIR}/target — much faster than rpmbuild clean builds"
    info "Includes all working-tree changes (committed or not)"

    (
        cd "$REPO_DIR"
        cargo build --release
    )

    if [[ ! -x "${REPO_DIR}/target/release/apexshot" ]]; then
        err "Build finished but target/release/apexshot is missing"
        exit 1
    fi
    if [[ ! -x "${REPO_DIR}/target/release/apexshot-capture" ]]; then
        err "Build finished but target/release/apexshot-capture is missing"
        err "The C++ capture helper must be produced by build.rs"
        exit 1
    fi

    ok "Local release build ready"
}

# Install into the same paths as packaging/fedora/apexshot.spec %install.
install_system_files() {
    step "Installing into system package paths"
    prime_sudo

    local bin_src="${REPO_DIR}/target/release/apexshot"
    local capture_src="${REPO_DIR}/target/release/apexshot-capture"

    if [[ ! -x "$bin_src" || ! -x "$capture_src" ]]; then
        err "Missing release binaries. Run without --no-build first."
        exit 1
    fi

    $SUDO install -Dm0755 "$bin_src" /usr/bin/apexshot
    $SUDO install -Dm0755 "$capture_src" /usr/bin/apexshot-capture
    $SUDO install -Dm0755 "${REPO_DIR}/packaging/deb/apexshot-native-host" /usr/bin/apexshot-native-host

    $SUDO install -Dm0644 "${REPO_DIR}/packaging/apexshot.desktop" \
        /usr/share/applications/io.github.codegoddy.apexshot.desktop
    $SUDO install -Dm0644 "${REPO_DIR}/packaging/apexshot-daemon.desktop" \
        /etc/xdg/autostart/apexshot.desktop
    $SUDO install -Dm0644 "${REPO_DIR}/packaging/apexshot.svg" \
        /usr/share/icons/hicolor/scalable/apps/apexshot.svg
    $SUDO install -Dm0644 "${REPO_DIR}/packaging/apexshot.svg" \
        /usr/share/icons/hicolor/scalable/apps/io.github.codegoddy.apexshot.svg
    $SUDO install -Dm0644 "${REPO_DIR}/packaging/apexshot.svg" \
        /usr/share/pixmaps/apexshot.svg

    $SUDO install -Dm0644 "${REPO_DIR}/native-host/io.github.codegoddy.apexshot.json" \
        /etc/opt/chrome/NativeMessagingHosts/io.github.codegoddy.apexshot.json
    $SUDO install -Dm0644 "${REPO_DIR}/native-host/io.github.codegoddy.apexshot.json" \
        /etc/chromium/NativeMessagingHosts/io.github.codegoddy.apexshot.json

    local extension_dir="/usr/share/gnome-shell/extensions/apexshot-gnome-integration@apexshot.github.io"
    $SUDO install -d "$extension_dir"
    local ext_file
    for ext_file in \
        metadata.json \
        extension.js \
        controls-ui.js \
        controls-ui-layout.js \
        runtime-overlays.js \
        runtime-overlays-visibility.js \
        mask-ui.js \
        window-list.js \
        session-state.js \
        screenshot-lock.js \
        gnome-version.js
    do
        $SUDO install -Dm0644 "${REPO_DIR}/gnome-extension/${ext_file}" \
            "${extension_dir}/${ext_file}"
    done

    local img
    for img in "${REPO_DIR}"/src/capture/editor/background-images/*.jpg; do
        [[ -f "$img" ]] || continue
        $SUDO install -Dm0644 "$img" \
            "/usr/share/apexshot/background-images/$(basename "$img")"
    done

    local snd
    for snd in "${REPO_DIR}"/assets/sounds/*.ogg; do
        [[ -f "$snd" ]] || continue
        $SUDO install -Dm0644 "$snd" \
            "/usr/share/apexshot/sounds/$(basename "$snd")"
    done

    if command -v update-desktop-database >/dev/null 2>&1; then
        $SUDO update-desktop-database -q /usr/share/applications 2>/dev/null || true
    fi
    if command -v gtk-update-icon-cache >/dev/null 2>&1; then
        $SUDO gtk-update-icon-cache -q /usr/share/icons/hicolor 2>/dev/null || true
    fi

    if command -v restorecon >/dev/null 2>&1; then
        $SUDO restorecon -v /usr/bin/apexshot /usr/bin/apexshot-capture 2>/dev/null || true
        ok "SELinux labels refreshed"
    fi

    if [[ ! -x /usr/bin/apexshot ]]; then
        err "Install finished but /usr/bin/apexshot is missing"
        exit 1
    fi

    ok "Installed system binaries: /usr/bin/apexshot (+ capture helper)"
}

# --- Optional slow RPM path -------------------------------------------------

create_source_archive() {
    local version=$1
    local archive=$2

    step "Packaging working tree as source archive"
    tar \
        --exclude-vcs \
        --exclude='./target' \
        --exclude='./.github' \
        --exclude='./.git' \
        --exclude='./node_modules' \
        --exclude='./test_gtk/target' \
        -czf "$archive" \
        --transform="s#^.#apexshot-${version}#" \
        -C "$REPO_DIR" .

    ok "Source archive: ${archive}"
}

build_rpm() {
    local version
    version="$(package_version)"

    step "Building Fedora RPM for ApexShot ${version}"
    info "Clean rebuild inside rpmbuild — can take many minutes"

    mkdir -p "${RPM_TOPDIR}"/{BUILD,BUILDROOT,RPMS,SOURCES,SPECS,SRPMS}

    local archive="${RPM_TOPDIR}/SOURCES/apexshot-${version}.tar.gz"
    create_source_archive "$version" "$archive"

    local spec="${RPM_TOPDIR}/SPECS/apexshot.spec"
    sed "s/^Version:.*/Version:        ${version}/" "$SPEC_SRC" > "$spec"

    rpmbuild \
        --define "_topdir ${RPM_TOPDIR}" \
        -ba "$spec"

    ok "RPM build finished under ${RPM_TOPDIR}/RPMS"
}

find_main_rpm() {
    local rpm
    rpm="$(
        find "${RPM_TOPDIR}/RPMS" -type f -name 'apexshot-*.rpm' \
            ! -name '*-debuginfo-*.rpm' \
            ! -name '*-debugsource-*.rpm' \
            -printf '%T@ %p\n' 2>/dev/null \
            | sort -nr \
            | head -n 1 \
            | cut -d' ' -f2-
    )"

    if [[ -z "$rpm" || ! -f "$rpm" ]]; then
        err "No installable apexshot RPM found under ${RPM_TOPDIR}/RPMS"
        exit 1
    fi

    printf '%s' "$rpm"
}

install_rpm() {
    local rpm_file=$1

    step "Installing package-managed ApexShot via dnf"
    info "Package: ${rpm_file}"
    prime_sudo

    if ! $SUDO dnf install -y "$rpm_file"; then
        info "install could not upgrade/replace; trying reinstall"
        $SUDO dnf reinstall -y "$rpm_file"
    fi

    if command -v restorecon >/dev/null 2>&1; then
        $SUDO restorecon -v /usr/bin/apexshot /usr/bin/apexshot-capture 2>/dev/null || true
    fi

    if [[ ! -x /usr/bin/apexshot ]]; then
        err "Install finished but /usr/bin/apexshot is missing"
        exit 1
    fi

    ok "Installed system package: /usr/bin/apexshot"
}

start_daemon_if_requested() {
    if [[ $START_DAEMON -ne 1 ]]; then
        return 0
    fi

    step "Starting ApexShot daemon"
    if ! command -v apexshot >/dev/null 2>&1; then
        err "apexshot not found on PATH after install"
        exit 1
    fi

    nohup apexshot daemon >/tmp/apexshot-daemon-reinstall.log 2>&1 &
    disown || true
    sleep 0.3
    if pgrep -x apexshot >/dev/null 2>&1; then
        ok "Daemon started (log: /tmp/apexshot-daemon-reinstall.log)"
    else
        warn "Daemon may not have started; check /tmp/apexshot-daemon-reinstall.log"
        warn "Or run: apexshot daemon"
    fi
}

summary() {
    local mode=$1
    local version
    version="$(package_version)"
    local rpm_state
    if rpm -q apexshot >/dev/null 2>&1; then
        rpm_state="$(rpm -q --qf '%{NAME}-%{VERSION}-%{RELEASE}' apexshot)"
    else
        rpm_state="file install (not tracked by rpm)"
    fi

    echo -e "\n${GREEN}${BOLD}═══════════════════════════════════════════════════════${RESET}"
    echo -e "${GREEN}${BOLD}  ApexShot removed, rebuilt, and installed (Fedora)${RESET}\n"
    echo -e "  Mode:      ${BOLD}${mode}${RESET}"
    echo -e "  Version:   ${BOLD}${version}${RESET}"
    echo -e "  Tracking:  ${BOLD}${rpm_state}${RESET}"
    echo -e "  Binary:    ${BOLD}/usr/bin/apexshot${RESET}"
    echo -e "  Capture:   ${BOLD}/usr/bin/apexshot-capture${RESET}"
    echo -e ""
    echo -e "  Next steps:"
    echo -e "    ${DIM}apexshot daemon${RESET}          # tray + hotkeys"
    echo -e "    ${DIM}apexshot${RESET}                 # settings UI"
    echo -e "    ${DIM}apexshot capture area${RESET}    # test a capture"
    echo -e ""
    echo -e "  Re-run after code changes:"
    echo -e "    ${DIM}./scripts/fedora-reinstall.sh${RESET}"
    echo -e "    ${DIM}./scripts/fedora-reinstall.sh --start${RESET}"
    echo -e "${GREEN}${BOLD}═══════════════════════════════════════════════════════${RESET}\n"
}

main() {
    parse_args "$@"

    echo -e "${CYAN}${BOLD}ApexShot Fedora reinstall${RESET}"
    if [[ $USE_RPM -eq 1 ]]; then
        info "Flow: remove → rpmbuild → dnf install (slow, full package)"
    else
        info "Flow: remove → cargo build --release → install to /usr (fast)"
    fi

    require_fedora
    setup_sudo

    if [[ $NO_STOP -eq 0 ]]; then
        stop_running_app
    else
        warn "Skipping process stop (--no-stop)"
    fi

    if [[ $NO_REMOVE -eq 0 ]]; then
        remove_installed_package
    else
        warn "Skipping package removal (--no-remove)"
    fi

    if [[ $USE_RPM -eq 1 ]]; then
        if [[ $NO_BUILD -eq 0 ]]; then
            ensure_build_tools
            build_rpm
        else
            step "Skipping build (--no-build)"
            ok "Using existing RPM artifacts"
        fi
        local rpm_file
        rpm_file="$(find_main_rpm)"
        install_rpm "$rpm_file"
        start_daemon_if_requested
        summary "rpm + dnf"
    else
        if [[ $NO_BUILD -eq 0 ]]; then
            ensure_build_tools
            build_local
        else
            step "Skipping build (--no-build)"
            ok "Using existing target/release binaries"
        fi
        install_system_files
        start_daemon_if_requested
        summary "local cargo + system paths"
    fi
}

main "$@"
