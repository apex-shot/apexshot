#!/usr/bin/env bash
# Smoke test for an installed ApexShot package: the binary starts, and short
# screen recordings through the real recorder produce playable MP4s. Needs
# xvfb-run and dbus-run-session; running as root is fine.
#
# Two recordings, both stopped with `apexshot record stop`: one through
# --overlay-stop, the path the app's own controls take, and one bare
# `record screen`, the path with no controls. Each runs under a hard timeout so
# a recorder that never stops fails the job instead of hanging it.
set -euo pipefail
out_dir=${1:-/tmp}

apexshot --version

# The recorder needs an X display; Xvfb stands in for a desktop session.
unset WAYLAND_DISPLAY

record_and_check() {
    local name=$1
    shift
    local out=$out_dir/apexshot-smoke-$name.mp4
    local log
    log=$(mktemp)
    rm -f "$out"

    local status=0
    timeout --kill-after=10 180 \
        xvfb-run -a -s "-screen 0 1280x720x24" dbus-run-session -- bash -c '
        set -uo pipefail
        out=$1
        log=$2
        shift 2
        apexshot record screen "$@" --output "$out" > "$log" 2>&1 &
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
    ' _ "$out" "$log" "$@" || status=$?

    echo "--- $name recorder output ---"
    cat "$log"
    if (( status != 0 )); then
        echo "::error::the $name smoke recording failed (exit $status)" >&2
        return 1
    fi

    local size
    size=$(stat -c %s "$out" 2>/dev/null || echo 0)
    if (( size < 10000 )); then
        echo "::error::$name recording missing or too small ($size bytes): $out" >&2
        return 1
    fi
    if command -v ffprobe >/dev/null; then
        local duration
        duration=$(ffprobe -v error -show_entries format=duration -of csv=p=0 "$out")
        echo "recorded $size bytes, ${duration}s ($name)"
        awk -v d="$duration" 'BEGIN { exit !(d >= 1.0) }' || {
            echo "::error::$name recording is shorter than one second (${duration}s)" >&2
            return 1
        }
    else
        echo "recorded $size bytes ($name; ffprobe not installed, duration not checked)"
    fi
}

record_and_check controls --overlay-stop
record_and_check standalone
