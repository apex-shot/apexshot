#!/usr/bin/env bash
# Compare the MSYS2 packages installed in this shell with the pinned versions
# in packaging/windows/msys2-versions.txt. Run inside an MSYS2 shell.
set -euo pipefail
pins=$(dirname "$0")/../packaging/windows/msys2-versions.txt
status=0
while read -r package expected; do
    [[ -z $package || $package == \#* ]] && continue
    actual=$(pacman -Q "$package" 2>/dev/null | awk '{print $2}')
    if [[ $actual != "$expected" ]]; then
        echo "::error::$package is ${actual:-not installed}, pinned $expected. MSYS2 moved: run the Windows smoke test with the new version, then update packaging/windows/msys2-versions.txt." >&2
        status=1
    else
        echo "$package $actual (pinned)"
    fi
done < "$pins"
exit $status
