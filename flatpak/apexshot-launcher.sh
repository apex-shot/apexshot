#!/bin/sh
set -eu

cache_dir=${XDG_CACHE_HOME:-${HOME:-/tmp}/.cache}
app_cache="$cache_dir/apexshot"
ocr_cache="$app_cache/ocr-models"
install -d -m 0700 "$app_cache" "$ocr_cache"

if [ -z "${QT_QPA_PLATFORM:-}" ]; then
    if [ -n "${WAYLAND_DISPLAY:-}" ]; then
        QT_QPA_PLATFORM=wayland
    else
        QT_QPA_PLATFORM=xcb
    fi
    export QT_QPA_PLATFORM
fi

seed_model() {
    source_path=$1
    target_path=$2
    if [ ! -s "$target_path" ]; then
        temporary_path="${target_path}.seed.$$"
        if cp "$source_path" "$temporary_path"; then
            mv -n "$temporary_path" "$target_path"
        fi
        rm -f "$temporary_path"
    fi
}

seed_model /app/share/apexshot/ocr-models/text-detection.rten "$ocr_cache/text-detection.rten"
seed_model /app/share/apexshot/ocr-models/text-recognition.rten "$ocr_cache/text-recognition.rten"
seed_model /app/share/apexshot/ocr-models/text-detection.rten "$app_cache/text-detection.rten"

exec /app/libexec/apexshot.bin "$@"
