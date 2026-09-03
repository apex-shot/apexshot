#!/bin/bash
set -e

# Pre-fetch Cargo dependencies so the codebase is ready to build.
# Idempotent and non-interactive.
cargo fetch

# Best-effort: make sure the lint/format tools CI expects are available.
# Never fails the hook.
ensure_lint_tools() {
    if command -v cargo-clippy >/dev/null 2>&1 && command -v cargo-fmt >/dev/null 2>&1; then
        return 0
    fi
    if command -v rustup >/dev/null 2>&1; then
        rustup component add rustfmt clippy 2>/dev/null || true
        return 0
    fi
    echo "clippy/rustfmt missing. Install the Rust linter and formatter:" >&2
    if command -v apt >/dev/null 2>&1; then
        echo "  sudo apt-get install -y clippy rustfmt    # or: rust-clippy on newer Ubuntu" >&2
    elif command -v dnf >/dev/null 2>&1; then
        echo "  sudo dnf install -y clippy rustfmt" >&2
    elif command -v pacman >/dev/null 2>&1; then
        echo "  sudo pacman -S --needed clippy rustfmt" >&2
    fi
}
ensure_lint_tools || true
