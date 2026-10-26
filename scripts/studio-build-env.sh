#!/usr/bin/env bash
# Studio configuration is read with option_env! at compile time, so a build
# without the pair ships with Studio unavailable. Official releases supply it
# from repository variables.

studio_build_env() {
    local repo_root env_key env_timestamp key timestamp source line name value first last
    repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
    env_key=${APEXSHOT_STUDIO_VERIFY_KEY:-}
    env_timestamp=${APEXSHOT_STUDIO_RELEASE_TIMESTAMP:-}

    if [[ -n "$env_key" && -n "$env_timestamp" ]]; then
        key=$env_key
        timestamp=$env_timestamp
        source=environment
    elif [[ -n "$env_key" || -n "$env_timestamp" ]]; then
        echo "error: APEXSHOT_STUDIO_VERIFY_KEY and APEXSHOT_STUDIO_RELEASE_TIMESTAMP must be set together." >&2
        return 1
    else
        key=
        timestamp=
        if [[ -f "$repo_root/.env" ]]; then
            while IFS= read -r line || [[ -n "$line" ]]; do
                line=${line%$'\r'}
                line=${line#"${line%%[![:space:]]*}"}
                [[ -z "$line" || "$line" == \#* ]] && continue
                case "$line" in
                    APEXSHOT_STUDIO_VERIFY_KEY=*)
                        name=key
                        ;;
                    APEXSHOT_STUDIO_RELEASE_TIMESTAMP=*)
                        name=timestamp
                        ;;
                    *)
                        continue
                        ;;
                esac
                value=${line#*=}
                value=${value#"${value%%[![:space:]]*}"}
                value=${value%"${value##*[![:space:]]}"}
                if [[ ${#value} -ge 2 ]]; then
                    first=${value:0:1}
                    last=${value: -1}
                    if [[ ( "$first" == "'" && "$last" == "'" ) || ( "$first" == '"' && "$last" == '"' ) ]]; then
                        value=${value:1:${#value}-2}
                    fi
                fi
                if [[ "$name" == key ]]; then
                    key=$value
                else
                    timestamp=$value
                fi
            done < "$repo_root/.env"
        fi

        if [[ -n "$key" && -n "$timestamp" ]]; then
            source=.env
        elif [[ -n "$key" || -n "$timestamp" ]]; then
            echo "error: .env must set APEXSHOT_STUDIO_VERIFY_KEY and APEXSHOT_STUDIO_RELEASE_TIMESTAMP together." >&2
            return 1
        elif command -v gh >/dev/null 2>&1; then
            key=$(gh api repos/apex-shot/apexshot/actions/variables \
                --jq '.variables[] | select(.name == "APEXSHOT_STUDIO_VERIFY_KEY") | .value' 2>/dev/null) || key=
            timestamp=$(gh api repos/apex-shot/apexshot/actions/variables \
                --jq '.variables[] | select(.name == "APEXSHOT_STUDIO_RELEASE_TIMESTAMP") | .value' 2>/dev/null) || timestamp=
            if [[ -n "$key" && -n "$timestamp" ]]; then
                source="repository variables"
            elif [[ -n "$key" || -n "$timestamp" ]]; then
                echo "error: GitHub repository variables must define APEXSHOT_STUDIO_VERIFY_KEY and APEXSHOT_STUDIO_RELEASE_TIMESTAMP together." >&2
                return 1
            fi
        fi
    fi

    if [[ -z "${key:-}" || -z "${timestamp:-}" ]]; then
        if [[ "${APEXSHOT_ALLOW_NO_STUDIO:-}" == 1 ]]; then
            echo "warning: building without Studio configuration by request (APEXSHOT_ALLOW_NO_STUDIO=1)." >&2
            return 0
        fi
        echo "error: APEXSHOT_STUDIO_VERIFY_KEY and APEXSHOT_STUDIO_RELEASE_TIMESTAMP are required; this build would ship without Studio. Add both to .env, export both, or read them with gh api repos/apex-shot/apexshot/actions/variables. Set APEXSHOT_ALLOW_NO_STUDIO=1 to opt out." >&2
        return 1
    fi

    if command -v python3 >/dev/null 2>&1; then
        if ! python3 "$repo_root/scripts/release_gate.py" validate-config "$key" "$timestamp"; then
            return 1
        fi
    else
        echo "warning: python3 is unavailable; skipping Studio configuration validation." >&2
    fi

    export APEXSHOT_STUDIO_VERIFY_KEY="$key"
    export APEXSHOT_STUDIO_RELEASE_TIMESTAMP="$timestamp"
    echo "Studio build configuration loaded from $source."
}

studio_build_env
