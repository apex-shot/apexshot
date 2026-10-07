#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

library="${APEXSHOT_GTK4_LAYER_SHELL_LIBRARY:-}"
if [[ -z "$library" ]]; then
    library=$(ldconfig -p 2>/dev/null | awk '$1 == "libgtk4-layer-shell.so.0" { print $NF; exit }')
fi
if [[ -z "$library" || ! -f "$library" ]]; then
    echo "libgtk4-layer-shell.so.0 is required to package the Debian runtime" >&2
    exit 1
fi

staging_dir="$ROOT_DIR/target/deb-staging"
mkdir -p "$staging_dir"
staged_library="$staging_dir/libgtk4-layer-shell.so.0"
cleanup() {
    rm -f "$staged_library"
}
trap cleanup EXIT
cp -L "$library" "$staged_library"
cargo deb "$@"
