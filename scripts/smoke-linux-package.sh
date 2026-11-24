#!/usr/bin/env bash
# Smoke test for an installed ApexShot package: the binary starts, and a short
# screen recording through the real recorder produces a playable MP4. Needs
# xvfb-run and dbus-run-session; run as a non-root user is not required.
set -euo pipefail
out=${1:-/tmp/apexshot-smoke.mp4}
rm -f "$out"

apexshot --version

# The recorder needs an X display; Xvfb stands in for a desktop session.
unset WAYLAND_DISPLAY
xvfb-run -a -s "-screen 0 1280x720x24" dbus-run-session -- bash -c '
    set -euo pipefail
    apexshot record screen --output "$1" &
    recorder=$!
    sleep 6
    apexshot record stop || kill -INT "$recorder"
    wait "$recorder"
' _ "$out"

size=$(stat -c %s "$out" 2>/dev/null || echo 0)
if (( size < 10000 )); then
    echo "::error::recording missing or too small ($size bytes): $out" >&2
    exit 1
fi
if command -v ffprobe >/dev/null; then
    duration=$(ffprobe -v error -show_entries format=duration -of csv=p=0 "$out")
    echo "recorded $size bytes, ${duration}s"
    awk -v d="$duration" 'BEGIN { exit !(d >= 1.0) }' || {
        echo "::error::recording is shorter than one second (${duration}s)" >&2
        exit 1
    }
else
    echo "recorded $size bytes (ffprobe not installed, duration not checked)"
fi
