#!/usr/bin/env bash
# Smoke tests for the ApexShot installer Worker + CDN mirror.
#
# NON-DESTRUCTIVE: the POSIX test executes the fetched script under dash with a
# stubbed PATH (no network, no sudo, no package manager), so nothing real can
# happen. Content is compared as bytes.
#
# Usage:
#   scripts/test-cloudflare-installer.sh
#   STRICT_SH=1      scripts/test-cloudflare-installer.sh   # POSIX issues = failure
#   NO_EXEC_TEST=1   scripts/test-cloudflare-installer.sh   # static lint only
#   WORKER=https://… scripts/test-cloudflare-installer.sh

set -uo pipefail

PUBLIC="${PUBLIC:-https://apexshot.org}"
WORKER="${WORKER:-https://apexshot-install.codegoddy.workers.dev}"
GH_RAW="https://raw.githubusercontent.com/apex-shot/apexshot/main/scripts"
STRICT_SH="${STRICT_SH:-0}"
NO_EXEC_TEST="${NO_EXEC_TEST:-0}"

PASS=0
FAIL=0
WARN=0

ok()   { printf '  \033[32m✔\033[0m  %s\n' "$1"; PASS=$((PASS + 1)); }
bad()  { printf '  \033[31m✖\033[0m  %s\n' "$1"; FAIL=$((FAIL + 1)); }
warn() { printf '  \033[33m⚠\033[0m  %s\n' "$1"; WARN=$((WARN + 1)); }
info() { printf '  \033[2m%s\033[0m\n' "$1"; }
sect() { printf '\n\033[1m%s\033[0m\n' "$1"; }

bust() { printf '%s?t=%s%s' "$1" "$(date +%s)" "${RANDOM}"; }

body()     { curl -fsSL --max-time 20 "$1" 2>/dev/null; }
code()     { curl -sS -o /dev/null -w '%{http_code}' --max-time 20 "$1" 2>/dev/null; }
hdrs()     { curl -sS -D - -o /dev/null --max-time 20 "$1" 2>/dev/null | tr -d '\r'; }
ctype()    { hdrs "$1" | awk 'tolower($1)=="content-type:"{print $2; exit}'; }
sourceof() { hdrs "$1" | awk 'tolower($1)=="x-apexshot-source:"{print $2; exit}'; }

printf '\033[1mApexShot installer Worker — smoke tests\033[0m\n'
printf 'public: %s\nworker: %s\n' "$PUBLIC" "$WORKER"

# ---------------------------------------------------------------------------
sect "0. Test environment"
# ---------------------------------------------------------------------------

info "/bin/sh -> $(readlink -f /bin/sh 2>/dev/null || printf '/bin/sh')"

# Probe empirically rather than inferring from the binary name: do NOT use
# `sh -c 'set -o pipefail'; echo $?` inside a printf — that reports printf's
# status, not the probe's. Hence the explicit two-step below.
pipefail_probe() {
    local bin=$1 label=$2
    "$bin" -c 'set -o pipefail' >/dev/null 2>&1
    local rc=$?
    if [[ $rc -eq 0 ]]; then
        info "$label accepts 'set -o pipefail' (rc=0)"
    else
        info "$label REJECTS 'set -o pipefail' (rc=$rc)"
    fi
}
pipefail_probe /bin/sh "/bin/sh"
command -v dash >/dev/null 2>&1 && pipefail_probe dash "dash"
command -v busybox >/dev/null 2>&1 && pipefail_probe "busybox sh" "busybox ash"

# Pick a strict POSIX shell for the execution test.
POSIX_SH=""
if command -v dash >/dev/null 2>&1; then
    POSIX_SH=dash
    info "strict POSIX shell for testing: $(command -v dash)"
elif command -v busybox >/dev/null 2>&1; then
    POSIX_SH=busybox
    info "strict POSIX shell for testing: busybox ash"
else
    warn "no dash/busybox available — POSIX execution test will be skipped"
fi

# ---------------------------------------------------------------------------
sect "1. Worker direct (isolates Worker logic from DNS/routing)"
# ---------------------------------------------------------------------------

url="$(bust "$WORKER/install")"

if [[ "$(code "$url")" == "200" ]]; then
    ok "GET /install -> 200"
else
    bad "GET /install -> $(code "$url") (expected 200)"
fi

scr="$(body "$url")"
if [[ "$scr" == '#'!* ]]; then
    ok "response begins with a shebang"
else
    bad "response does not begin with a shebang: ${scr:0:40}"
fi

if [[ "$(ctype "$url")" == "text/x-shellscript;"* ]]; then
    ok "content-type is text/x-shellscript"
else
    warn "content-type is '$(ctype "$url")' (expected text/x-shellscript)"
fi

src="$(sourceof "$url")"
case "$src" in
    r2)     ok "served from R2 mirror (x-apexshot-source: r2)" ;;
    github) ok "served from GitHub fallback (x-apexshot-source: github)" ;;
    "")     warn "no x-apexshot-source header (older Worker version deployed?)" ;;
    *)      warn "unexpected x-apexshot-source: $src" ;;
esac

# ---------------------------------------------------------------------------
sect "2. Routes on apexshot.org"
# ---------------------------------------------------------------------------

url="$(bust "$PUBLIC/install")"
if [[ "$(code "$url")" == "200" ]]; then
    ok "GET $PUBLIC/install -> 200 (route registered)"
else
    bad "GET $PUBLIC/install -> $(code "$url") (expected 200 — is the route active?)"
fi

pub_scr="$(body "$url")"
if [[ "$pub_scr" == '<!DOCTYPE'* || "$pub_scr" == '<html'* ]]; then
    bad "route returned HTML — the marketing site answered, not the Worker"
elif [[ -n "$pub_scr" ]]; then
    ok "route returns a shell script, not the website"
else
    bad "route returned an empty body"
fi

root_code="$(code "$PUBLIC/")"
if [[ "$root_code" == "200" || "$root_code" == "301" || "$root_code" == "302" ]]; then
    ok "GET $PUBLIC/ -> $root_code (site unaffected)"
else
    bad "GET $PUBLIC/ -> $root_code (the route may be swallowing the zone)"
fi

# ---------------------------------------------------------------------------
sect "3. Per-distro entrypoints and error paths"
# ---------------------------------------------------------------------------

for p in /install/ubuntu /install/arch /install/fedora /update /update/ubuntu /update/arch /update/fedora; do
    c="$(code "$(bust "$PUBLIC$p")")"
    if [[ "$c" == "200" ]]; then
        ok "GET $p -> 200"
    else
        bad "GET $p -> $c (expected 200)"
    fi
done

ts="$(code "$(bust "$PUBLIC/install/")")"
if [[ "$ts" == "200" ]]; then
    ok "GET /install/ -> 200 (trailing slash normalised)"
else
    bad "GET /install/ -> $ts (expected 200)"
fi

unknown="$(code "$(bust "$PUBLIC/install/does-not-exist")")"
if [[ "$unknown" == "404" ]]; then
    ok "GET /install/does-not-exist -> 404"
else
    bad "GET /install/does-not-exist -> $unknown (expected 404)"
fi

post="$(curl -sS -o /dev/null -w '%{http_code}' -X POST --max-time 20 "$(bust "$PUBLIC/install")" 2>/dev/null)"
if [[ "$post" == "405" ]]; then
    ok "POST /install -> 405"
else
    warn "POST /install -> $post (expected 405)"
fi

# ---------------------------------------------------------------------------
sect "4. Content integrity"
# ---------------------------------------------------------------------------

if [[ "$src" == "github" ]]; then
    gh="$(curl -fsSL --max-time 20 "$GH_RAW/install.sh" 2>/dev/null)"
    if [[ "$scr" == "$gh" ]]; then
        ok "body is byte-identical to GitHub main"
    else
        bad "body differs from GitHub main (stale cache?)"
    fi
else
    warn "skipping byte comparison (served from R2, not GitHub)"
fi

# ---------------------------------------------------------------------------
sect "5. Shell compatibility ('curl … | sh')"
# ---------------------------------------------------------------------------

tmp="$(mktemp)"
printf '%s' "$scr" > "$tmp"

# --- 5a. Syntax check: necessary, NOT sufficient ---------------------------
if bash -n "$tmp" 2>/dev/null; then
    ok "parses under bash (bash -n)"
else
    bad "does not parse under bash — recent deploy is broken"
    bash -n "$tmp" 2>&1 | sed 's/^/      /'
fi

# `dash -n` checks SYNTAX ONLY. It performs no expansion, so runtime failures
# like `set -o pipefail` and bad substitutions (`${var,,}`) pass straight
# through. A pass here is informational, never proof.
if [[ -n "$POSIX_SH" ]]; then
    if [[ "$POSIX_SH" == "dash" ]]; then
        dash -n "$tmp" >/dev/null 2>&1
    else
        busybox sh -n "$tmp" >/dev/null 2>&1
    fi
    if [[ $? -eq 0 ]]; then
        info "syntax-only pass under $POSIX_SH (NOT proof of POSIX compatibility)"
    else
        bad "syntax error under $POSIX_SH"
    fi
fi

# --- 5b. Authoritative: EXECUTE under a strict POSIX shell -----------------
# Commands that could touch the system or network are stubbed, so the script
# cannot do anything real however far it gets. It dies at the first bashism —
# exactly what a real `curl | sh` user experiences.
dash_exec_verdict=""
if [[ "$NO_EXEC_TEST" == "1" ]]; then
    info "execution test skipped (NO_EXEC_TEST=1)"
elif [[ -z "$POSIX_SH" ]]; then
    info "execution test skipped (no strict POSIX shell available)"
else
    stub="$(mktemp -d)"
    for c in curl wget sudo apt apt-get dpkg pacman dnf zypper bash sh gnome-extensions \
             unzip id getent systemctl runuser nohup killall makepkg clear; do
        # `sh` is stubbed too, so an `exec sh -c` handoff cannot recurse into
        # the real shell and surprise us.
        printf '#!/bin/sh\necho "STUB:%s called" >&2\nexit 1\n' "$c" > "$stub/$c"
        chmod +x "$stub/$c"
    done

    if [[ "$POSIX_SH" == "dash" ]]; then
        out="$(env -i PATH="$stub:/usr/bin:/bin" HOME="$stub" dash "$tmp" 2>&1)"; rc=$?
        label="dash"
    else
        out="$(env -i PATH="$stub:/usr/bin:/bin" HOME="$stub" busybox sh "$tmp" 2>&1)"; rc=$?
        label="busybox ash"
    fi

    if printf '%s\n' "$out" | grep -qE 'Illegal option|Bad substitution'; then
        dash_exec_verdict="incompatible"
        warn "EXECUTING under $label fails immediately — 'curl … | sh' is broken today"
        printf '%s\n' "$out" | grep -nE 'Illegal option|Bad substitution' | head -n 3 | sed 's/^/        /'
    elif [[ $rc -ne 0 ]]; then
        dash_exec_verdict="other"
        info "ran under $label, exited rc=$rc at a stubbed command (not a bashism)"
        printf '%s\n' "$out" | grep -v '^STUB:' | head -n 3 | sed 's/^/        /'
    else
        dash_exec_verdict="clean"
        ok "executes cleanly under $label (all commands stubbed)"
    fi

    rm -rf "$stub"
fi

# --- 5c. Static lint: host-independent truth ------------------------------
DASH_LINT_LABELS=(
    'array assignment (name=(...))'
    '${var,,} / ${var^^} case expansion (bash 4)'
    '[[ ... ]] test keyword'
    'source builtin (dash has only .)'
    'BASH_SOURCE'
    'function keyword'
    "ANSI-C \$'...' quoting"
    '&> redirection'
    'readarray / mapfile'
    'declare -A'
)
DASH_LINT_REGEXES=(
    '^[[:space:]]*[A-Za-z_][A-Za-z0-9_]*(\+)?=\('
    '\$\{[A-Za-z_][A-Za-z0-9_]*(\[[^]]*\])?[,^]'
    '\[\['
    '(^|[;&|(])[[:space:]]*source[[:space:]]'
    'BASH_SOURCE'
    '^[[:space:]]*function[[:space:]]'
    "\$'"
    '&>'
    '(readarray|mapfile)'
    'declare[[:space:]]+-A'
)

DASH_HITS=0
for i in "${!DASH_LINT_LABELS[@]}"; do
    label="${DASH_LINT_LABELS[$i]}"
    re="${DASH_LINT_REGEXES[$i]}"
    found="$(grep -nE "$re" "$tmp" 2>/dev/null | head -n 2)"
    [[ -z "$found" ]] && continue
    DASH_HITS=$((DASH_HITS + 1))
    warn "dash-incompatible: ${label}"
    printf '%s\n' "$found" | sed 's/^/        /'
done

if [[ "$DASH_HITS" -eq 0 && "$dash_exec_verdict" == "clean" ]]; then
    ok "POSIX-clean — 'curl … | sh' is safe to advertise"
elif [[ "$DASH_HITS" -eq 0 ]]; then
    info "no lint hits (execution test inconclusive on this host)"
else
    info "see docs/CLOUDFLARE_CDN_SETUP.md step 9 for the POSIX rewrite"
fi

rm -f "$tmp"

# ---------------------------------------------------------------------------
printf '\n\033[1mResult:\033[0m %d passed, %d failed, %d warnings\n' "$PASS" "$FAIL" "$WARN"

hard_fail="$FAIL"
if [[ "$STRICT_SH" == "1" && ( "$DASH_HITS" -gt 0 || "$dash_exec_verdict" == "incompatible" ) ]]; then
    printf '\033[31mSTRICT_SH=1 and the entrypoint is not POSIX-clean — blocking.\033[0m\n'
    hard_fail=$((hard_fail + 1))
fi

if [[ "$hard_fail" -gt 0 ]]; then
    printf '\033[31mFAILED\033[0m\n'
    exit 1
fi
printf '\033[32mOK\033[0m\n'
