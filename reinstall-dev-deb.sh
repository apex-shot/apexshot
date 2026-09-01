#!/usr/bin/env bash
# One command: incremental .deb build, then purge + install for local testing.
#   ./reinstall-dev-deb.sh
#
# Does not cargo clean. Cargo reuses crates. Stages the capture helper from
# the just-built release binary before cargo-deb packages it.
set -euo pipefail

PACKAGE_NAME="apexshot"
ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

cd "$ROOT_DIR"

if ! command -v cargo >/dev/null 2>&1; then
  echo "cargo is not installed or not on PATH" >&2
  exit 1
fi
if ! command -v dpkg >/dev/null 2>&1; then
  echo "dpkg is not installed or not on PATH" >&2
  exit 1
fi
if ! command -v apt >/dev/null 2>&1; then
  echo "apt is not installed or not on PATH" >&2
  exit 1
fi
if ! cargo deb --version >/dev/null 2>&1; then
  echo "cargo-deb is not installed. Install with: cargo install cargo-deb" >&2
  exit 1
fi

# CI gates on `cargo fmt` and `cargo clippy`; make sure both are present.
# Ubuntu ships clippy as "clippy" (24.04) or "rust-clippy" (newer releases).
if ! command -v cargo-clippy >/dev/null 2>&1 || ! command -v cargo-fmt >/dev/null 2>&1; then
  echo "Installing Rust lint/format tools (clippy, rustfmt)..."
  CLIPPY_PKG="clippy"
  apt-cache show clippy >/dev/null 2>&1 || CLIPPY_PKG="rust-clippy"
  sudo apt-get install -y "$CLIPPY_PKG" rustfmt
fi

export CARGO_INCREMENTAL=1

echo "Building ApexShot .deb..."
echo "→ incremental cargo release"
cargo build --release

if [[ ! -x "$ROOT_DIR/target/release/apexshot" ]]; then
  echo "error: target/release/apexshot is missing after build" >&2
  exit 1
fi
if [[ ! -x "$ROOT_DIR/target/release/apexshot-capture" ]]; then
  echo "error: target/release/apexshot-capture is missing after build" >&2
  echo "The C++ capture helper must be produced by build.rs" >&2
  exit 1
fi

echo "Staging capture helper..."
cp "$ROOT_DIR/target/release/apexshot-capture" "$ROOT_DIR/packaging/deb/apexshot-capture"
cmp "$ROOT_DIR/target/release/apexshot-capture" "$ROOT_DIR/packaging/deb/apexshot-capture"

echo "→ cargo-deb (reuse existing release binaries)"
cargo deb --no-build

shopt -s nullglob
deb_files=("$ROOT_DIR"/target/debian/apexshot_*.deb)
shopt -u nullglob
if [ "${#deb_files[@]}" -eq 0 ]; then
  echo "No .deb file found after build" >&2
  exit 1
fi
newest_deb="${deb_files[0]}"
for candidate in "${deb_files[@]}"; do
  [ "$candidate" -nt "$newest_deb" ] && newest_deb="$candidate"
done
echo "Built package: $newest_deb"

apexshot_is_running() {
  pgrep -x apexshot >/dev/null 2>&1 \
    || pgrep -x apexshot-captur >/dev/null 2>&1 \
    || pgrep -x apexshot-capture >/dev/null 2>&1
}

wait_for_apexshot_exit() {
  local attempts="$1"
  local attempt
  for ((attempt = 0; attempt < attempts; attempt++)); do
    if ! apexshot_is_running; then
      return 0
    fi
    sleep 0.25
  done
  return 1
}

echo "Stopping running ApexShot processes..."
pkill -x apexshot 2>/dev/null || true
# Linux truncates the helper's process name to 15 characters.
pkill -x apexshot-captur 2>/dev/null || true
pkill -x apexshot-capture 2>/dev/null || true
if ! wait_for_apexshot_exit 20; then
  echo "ApexShot did not exit after 5 seconds; forcing shutdown..."
  pkill -9 -x apexshot 2>/dev/null || true
  pkill -9 -x apexshot-captur 2>/dev/null || true
  pkill -9 -x apexshot-capture 2>/dev/null || true
  if ! wait_for_apexshot_exit 8; then
    echo "Error: ApexShot processes did not stop" >&2
    ps -eo pid,comm,args | grep -E '[a]pexshot(-captur(e)?)?' >&2 || true
    exit 1
  fi
fi

echo "Requesting sudo once for uninstall/install..."
sudo -v

# PackageKit is D-Bus activated on desktop systems and can hold dpkg's lock
# while checking for updates. Stop it before changing the package database;
# never remove the lock files themselves.
echo "Stopping PackageKit so dpkg can acquire its lock..."
sudo systemctl stop packagekit.service
for lock_file in /var/lib/dpkg/lock-frontend /var/lib/dpkg/lock; do
  for attempt in {1..20}; do
    if ! sudo fuser "$lock_file" >/dev/null 2>&1; then
      break
    fi
    if [ "$attempt" -eq 20 ]; then
      echo "error: $lock_file is still held after stopping PackageKit" >&2
      sudo fuser -v "$lock_file" >&2 || true
      exit 1
    fi
    sleep 0.25
  done
done

pkg_status="$(dpkg-query -W -f='${Status}' "$PACKAGE_NAME" 2>/dev/null || true)"
if [ -n "$pkg_status" ]; then
  echo "Purging $PACKAGE_NAME (status: $pkg_status)..."
  sudo dpkg -P "$PACKAGE_NAME" || sudo dpkg -P --force-all "$PACKAGE_NAME"
else
  echo "$PACKAGE_NAME is not currently installed; skipping removal."
fi
echo "Installing $newest_deb..."
sudo apt install -y --reinstall --allow-downgrades "$newest_deb"

echo "Verifying installed binaries..."
cmp "$ROOT_DIR/target/release/apexshot" /usr/bin/apexshot
cmp "$ROOT_DIR/target/release/apexshot-capture" /usr/bin/apexshot-capture

EXT_UUID="apexshot-gnome-integration@apexshot.github.io"
USER_EXT="$HOME/.local/share/gnome-shell/extensions/$EXT_UUID"
EXT_FILES=(metadata.json extension.js shell-overlay.js window-list.js preview-stacking.js)
extension_changed=false
for file in "${EXT_FILES[@]}"; do
  if [[ ! -f "$USER_EXT/$file" ]] \
    || ! cmp -s "$ROOT_DIR/gnome-extension/$file" "$USER_EXT/$file"; then
    extension_changed=true
    break
  fi
done

if [[ "$extension_changed" == true ]]; then
  echo "Updating GNOME Shell extension (source changed)..."
  mkdir -p "$USER_EXT"
  for file in "${EXT_FILES[@]}"; do
    cp -a "$ROOT_DIR/gnome-extension/$file" "$USER_EXT/$file"
  done
  if command -v gnome-extensions >/dev/null 2>&1; then
    gnome-extensions disable "$EXT_UUID" 2>/dev/null || true
    gnome-extensions enable "$EXT_UUID" 2>/dev/null || true
  fi
else
  echo "GNOME Shell extension is unchanged; skipping copy and reload."
fi

echo "Installed $PACKAGE_NAME from $newest_deb"
