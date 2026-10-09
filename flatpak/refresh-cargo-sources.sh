#!/usr/bin/env bash
set -euo pipefail

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
generator_commit=74697c75b630d7330e77250fc13cb5ea688d9479
generator_sha256=0a2db6be87d75910facef28ab46d4d6460802e8419ab850d0caa6a364d26b380
temporary_directory=$(mktemp -d)
trap 'rm -rf "$temporary_directory"' EXIT

generator="$temporary_directory/flatpak-cargo-generator.py"
curl -fsSL --proto '=https' --tlsv1.2 \
    "https://raw.githubusercontent.com/flatpak/flatpak-builder-tools/${generator_commit}/cargo/flatpak-cargo-generator.py" \
    -o "$generator"
printf '%s  %s\n' "$generator_sha256" "$generator" | sha256sum --check --status
uv run --no-project "$generator" "$repo_root/Cargo.lock" \
    --output "$repo_root/flatpak/cargo-sources.json"
