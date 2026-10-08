#!/usr/bin/env bash
# One command: incremental .deb build, then purge + install for local testing.
#   ./reinstall-dev-deb.sh
#
# Does not cargo clean. Cargo reuses crates. Stages the capture helper from
# the just-built release binary before cargo-deb packages it.
set -euo pipefail

PACKAGE_NAME="apexshot"
ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
START_SECONDS=$SECONDS

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

# Release incremental is ON here because this is the local dev loop. `apexshot`
# is one big crate, so any source change otherwise recodes all ~536 units at
# opt-level 3: minutes per run. Incremental reuses the previous codegen instead
# (measured ~3m20s -> ~6s on this machine). It once tripped a rustc link bug
# (`undefined reference to core::ptr::drop_in_place<...>` out of tokio/zbus)
# when stale incremental units were reused after a source change; current rustc
# no longer reproduces it. The build below still guards against it: on failure
# it drops only the release incremental cache and rebuilds clean once. Set
# APEXSHOT_RELEASE_INCREMENTAL=0 to force the old non-incremental behaviour.
if [[ "${APEXSHOT_RELEASE_INCREMENTAL:-1}" != "0" ]]; then
  export CARGO_INCREMENTAL=1
else
  export CARGO_INCREMENTAL=0
fi

# Cargo's committed config pins `jobs = 4` to keep memory in check, and cargo
# hands those same tokens to rustc, which caps how many codegen units the
# compiler may build in parallel. For a single-crate release build that cap is
# the whole bill: measured ~185s at 4 tokens vs ~135s at 20 on this machine.
# Scale it to the machine, with a RAM ceiling so a cold build can't OOM.
# CARGO_BUILD_JOBS from the environment still wins if you set it.
default_cargo_jobs() {
  local cpus mem_jobs jobs
  cpus="$(nproc 2>/dev/null || echo 4)"
  mem_jobs="$(awk '/^MemAvailable:/ { printf "%d", $2 / 1024 / 1024 / 2 }' /proc/meminfo 2>/dev/null || echo 4)"
  [[ "$mem_jobs" =~ ^[0-9]+$ ]] || mem_jobs=4
  jobs=$((cpus < mem_jobs ? cpus : mem_jobs))
  if ((jobs < 4)); then jobs=4; fi
  if ((jobs > 16)); then jobs=16; fi
  printf '%s\n' "$jobs"
}
cargo_jobs_args=()
if [[ -z "${CARGO_BUILD_JOBS:-}" ]]; then
  cargo_jobs_args=(-j "$(default_cargo_jobs)")
fi

echo "Building ApexShot .deb..."
if [[ "$CARGO_INCREMENTAL" == "1" ]]; then
  echo "→ cargo release (incremental; see note above)"
else
  echo "→ cargo release (non-incremental, forced by APEXSHOT_RELEASE_INCREMENTAL=0)"
fi
build_start=$SECONDS
if ! cargo build --release "${cargo_jobs_args[@]}"; then
  # The historical failure mode: stale incremental units leave dangling
  # `drop_in_place` symbols at link time. Drop just the release incremental
  # cache (never the whole target/) and rebuild clean for this run.
  echo "warning: release build failed; clearing the incremental cache and retrying clean..." >&2
  rm -rf "$ROOT_DIR/target/release/incremental"
  CARGO_INCREMENTAL=0 cargo build --release "${cargo_jobs_args[@]}"
fi
build_seconds=$((SECONDS - build_start))

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
deb_start=$SECONDS
# `--fast` trades .deb size for speed; this package is for local testing, and
# xz at the default level is ~1/6 of the run's remaining wall clock.
cargo deb --no-build --fast
deb_seconds=$((SECONDS - deb_start))

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
install_start=$SECONDS
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
apt_deb="$(mktemp --tmpdir --suffix=.deb apexshot-dev-deb.XXXXXX)"
trap 'rm -f "$apt_deb"' EXIT
install -m 0644 "$newest_deb" "$apt_deb"
sudo apt install -y --reinstall --allow-downgrades "$apt_deb"
rm -f "$apt_deb"
trap - EXIT
install_seconds=$((SECONDS - install_start))

echo "Verifying installed binaries..."
cmp "$ROOT_DIR/target/release/apexshot" /usr/bin/apexshot
cmp "$ROOT_DIR/target/release/apexshot-capture" /usr/bin/apexshot-capture

EXT_UUID="apexshot-gnome-integration@apexshot.github.io"
SYSTEM_EXT="/usr/share/gnome-shell/extensions/$EXT_UUID"
USER_EXT="$HOME/.local/share/gnome-shell/extensions/$EXT_UUID"
EXT_FILES=(metadata.json extension.js cursor-classifier.js press-tracker.js shell-overlay.js window-list.js preview-stacking.js daemon-ownership.js)

echo "Verifying packaged GNOME Shell extension..."
for file in "${EXT_FILES[@]}"; do
  if ! cmp -s "$ROOT_DIR/gnome-extension/$file" "$SYSTEM_EXT/$file"; then
    echo "error: installed GNOME extension file does not match source: $file" >&2
    exit 1
  fi
done

echo "Refreshing live GNOME Shell extension..."
if command -v gnome-extensions >/dev/null 2>&1; then
  gnome-extensions disable "$EXT_UUID" 2>/dev/null || true
fi
mkdir -p "$USER_EXT"
for file in "${EXT_FILES[@]}"; do
  cp -a "$ROOT_DIR/gnome-extension/$file" "$USER_EXT/$file"
  cmp "$ROOT_DIR/gnome-extension/$file" "$USER_EXT/$file"
done
if command -v gnome-extensions >/dev/null 2>&1; then
  gnome-extensions enable "$EXT_UUID"
fi

echo "Installed $PACKAGE_NAME from $newest_deb"
echo "Timings: total $((SECONDS - START_SECONDS))s | build ${build_seconds}s | package ${deb_seconds}s | install ${install_seconds}s"
