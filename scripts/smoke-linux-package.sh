#!/usr/bin/env bash
# Smoke test for an installed ApexShot package: the binary starts, and a short
# screen recording through the real recorder produces a playable MP4. Needs
# xvfb-run and dbus-run-session; run as a non-root user is not required.
set -euo pipefail
out=${1:-/tmp/apexshot-smoke.mp4}
rm -f "$out"

apexshot --version

# The recorder needs an X display; Xvfb stands in for a desktop session.
# --overlay-stop is the path the app's own controls take: it serves the D-Bus
# control that `apexshot record stop` reaches. A bare `record screen` has no
# way to be stopped. Everything runs under a hard timeout so a recorder that
# never stops fails the job instead of hanging it.
unset WAYLAND_DISPLAY
log=$(mktemp)
status=0
timeout --kill-after=10 180 \
    xvfb-run -a -s "-screen 0 1280x720x24" dbus-run-session -- bash -c '
    set -uo pipefail
    apexshot record screen --overlay-stop --output "$1" > "$2" 2>&1 &
    recorder=$!
    # Startup plus any countdown, then a few seconds of recording.
    sleep 10
    stopped=0
    for _ in $(seq 30); do
        if apexshot record stop; then stopped=1; break; fi
        kill -0 "$recorder" 2>/dev/null || break
        sleep 1
    done
    if (( ! stopped )); then
        echo "::error::the recording could not be stopped through its control" >&2
        kill -TERM "$recorder" 2>/dev/null
        exit 1
    fi
    for _ in $(seq 60); do
        kill -0 "$recorder" 2>/dev/null || break
        sleep 1
    done
    wait "$recorder"
' _ "$out" "$log" || status=$?
echo "--- recorder output ---"
cat "$log"
if (( status != 0 )); then
    echo "::error::the smoke recording failed (exit $status)" >&2
    exit 1
fi

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
