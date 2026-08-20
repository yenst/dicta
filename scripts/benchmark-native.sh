#!/usr/bin/env bash

set -uo pipefail

script_dir=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd -P)
repo_root=$(cd -- "$script_dir/.." && pwd -P)
native_binary="$repo_root/apps/dicta-native/build/dicta-native"
tauri_binary="$repo_root/src-tauri/target/release/dicta"
runs=6
idle_seconds=2
runtime_mode=auto
build_native=false
active_pid=
temp_dir=

usage() {
    cat <<'EOF'
Usage: scripts/benchmark-native.sh [options]

Options:
  --native PATH       Native Qt executable to inspect
  --tauri PATH        Existing Tauri executable to compare (never built here)
  --runs N            Window-map samples; first is reported separately (default: 6)
  --idle-seconds N    Seconds before RSS sampling (default: 2)
  --runtime MODE      auto, gui, headless, or skip (default: auto)
  --build-native      Configure and build the native Qt Release target first
  -h, --help          Show this help

The report is written to stdout as Markdown. No package manager or network
command is invoked. GUI startup uses Hyprland's mapped-window event as the
event-loop proxy. Headless mode can measure RSS, but not startup-to-window-map.
EOF
}

die() {
    printf 'benchmark-native: %s\n' "$*" >&2
    exit 2
}

is_positive_integer() {
    [[ $1 =~ ^[1-9][0-9]*$ ]]
}

while (($# > 0)); do
    case $1 in
        --native)
            (($# >= 2)) || die "--native requires a path"
            native_binary=$2
            shift 2
            ;;
        --tauri)
            (($# >= 2)) || die "--tauri requires a path"
            tauri_binary=$2
            shift 2
            ;;
        --runs)
            (($# >= 2)) || die "--runs requires a positive integer"
            is_positive_integer "$2" || die "--runs must be a positive integer"
            runs=$2
            shift 2
            ;;
        --idle-seconds)
            (($# >= 2)) || die "--idle-seconds requires a positive integer"
            is_positive_integer "$2" || die "--idle-seconds must be a positive integer"
            idle_seconds=$2
            shift 2
            ;;
        --runtime)
            (($# >= 2)) || die "--runtime requires auto, gui, headless, or skip"
            runtime_mode=$2
            case $runtime_mode in
                auto|gui|headless|skip) ;;
                *) die "--runtime requires auto, gui, headless, or skip" ;;
            esac
            shift 2
            ;;
        --build-native)
            build_native=true
            shift
            ;;
        -h|--help)
            usage
            exit 0
            ;;
        *) die "unknown option: $1" ;;
    esac
done

cleanup_child() {
    if [[ -n ${active_pid:-} ]] && kill -0 "$active_pid" 2>/dev/null; then
        kill -TERM "$active_pid" 2>/dev/null || true
        for _ in {1..20}; do
            kill -0 "$active_pid" 2>/dev/null || break
            sleep 0.05
        done
        if kill -0 "$active_pid" 2>/dev/null; then
            kill -KILL "$active_pid" 2>/dev/null || true
        fi
        wait "$active_pid" 2>/dev/null || true
    fi
    active_pid=
}

cleanup() {
    cleanup_child
    if [[ -n ${temp_dir:-} ]]; then
        rm -f -- "$temp_dir/runtime.log"
        rmdir -- "$temp_dir" 2>/dev/null || true
    fi
}

on_signal() {
    cleanup
    trap - EXIT
    exit 130
}

trap cleanup EXIT
trap on_signal INT TERM

for required_tool in awk date env file ldd mktemp paste readelf rm rmdir sed sha256sum sleep sort stat uname wc; do
    command -v "$required_tool" >/dev/null 2>&1 \
        || die "required measurement tool is missing: $required_tool"
done

if $build_native; then
    command -v cmake >/dev/null 2>&1 || die "cmake is required by --build-native"
    cmake -S "$repo_root/apps/dicta-native" \
        -B "$repo_root/apps/dicta-native/build" \
        -DCMAKE_BUILD_TYPE=Release >&2 || die "native CMake configure failed"
    cmake --build "$repo_root/apps/dicta-native/build" --config Release >&2 \
        || die "native Release build failed"
fi

[[ -f $native_binary && -x $native_binary ]] \
    || die "native executable is missing or not executable: $native_binary"

temp_dir=$(mktemp -d "${TMPDIR:-/tmp}/dicta-native-benchmark.XXXXXX") \
    || die "could not create temporary measurement directory"
runtime_log="$temp_dir/runtime.log"

human_bytes() {
    if command -v numfmt >/dev/null 2>&1; then
        numfmt --to=iec-i --suffix=B "$1"
    else
        printf '%s bytes' "$1"
    fi
}

median() {
    local -a sorted
    mapfile -t sorted < <(printf '%s\n' "$@" | sort -n)
    local count=${#sorted[@]}
    ((count > 0)) || return 1
    if ((count % 2 == 1)); then
        printf '%s\n' "${sorted[count / 2]}"
    else
        awk -v a="${sorted[count / 2 - 1]}" -v b="${sorted[count / 2]}" \
            'BEGIN { printf "%.1f\n", (a + b) / 2 }'
    fi
}

emit_binary_facts() {
    local label=$1
    local binary=$2
    local bytes sha direct_count transitive_count missing
    bytes=$(stat -c '%s' -- "$binary") || return 1
    sha=$(sha256sum -- "$binary" | awk '{print $1}')
    mapfile -t direct_libraries < <(
        readelf -d -- "$binary" 2>/dev/null \
            | sed -n 's/.*Shared library: \[\([^]]*\)\].*/\1/p' \
            | sort -u
    )
    direct_count=${#direct_libraries[@]}
    transitive_count=$(ldd -- "$binary" 2>/dev/null | sed '/^[[:space:]]*$/d' | wc -l)
    missing=$(ldd -- "$binary" 2>/dev/null \
        | awk '/not found/ {print $1}' \
        | paste -sd ',' - \
        | sed 's/,/, /g')

    printf '### %s\n\n' "$label"
    printf -- '- Artifact: `%s`\n' "$binary"
    printf -- '- File size: %s bytes (%s)\n' "$bytes" "$(human_bytes "$bytes")"
    printf -- '- SHA-256: `%s`\n' "$sha"
    printf -- '- ELF description: `%s`\n' "$(file -b -- "$binary")"
    printf -- '- Direct ELF dependencies: %s\n' "$direct_count"
    printf -- '- Runtime dependency lines from `ldd`: %s\n' "$transitive_count"
    if [[ -n $missing ]]; then
        printf -- '- Unresolved dependencies: `%s`\n' "$missing"
    else
        printf -- '- Unresolved dependencies: none\n'
    fi
    printf -- '- Direct dependency names:\n'
    if ((direct_count == 0)); then
        printf '  - none detected\n'
    else
        local library
        for library in "${direct_libraries[@]}"; do
            printf '  - `%s`\n' "$library"
        done
    fi
    printf '\n'
}

rss_kib() {
    awk '/^VmRSS:/ {print $2; exit}' "/proc/$1/status" 2>/dev/null
}

hyprland_probe_available() {
    command -v hyprctl >/dev/null 2>&1 \
        && command -v jq >/dev/null 2>&1 \
        && [[ -n ${HYPRLAND_INSTANCE_SIGNATURE:-} ]] \
        && hyprctl -j clients 2>/dev/null | jq -e 'type == "array"' >/dev/null 2>&1
}

start_native() {
    local mode=$1
    : >"$runtime_log"
    if [[ $mode == headless ]]; then
        env -u QT_QPA_PLATFORMTHEME -u QT_STYLE_OVERRIDE \
            QT_QPA_PLATFORM=offscreen "$native_binary" >"$runtime_log" 2>&1 &
    else
        "$native_binary" >"$runtime_log" 2>&1 &
    fi
    active_pid=$!
}

measure_headless_rss() {
    local -a samples=()
    local value
    start_native headless
    sleep 0.3
    if ! kill -0 "$active_pid" 2>/dev/null; then
        printf 'not measured (offscreen process exited; see runtime diagnostics below)'
        cleanup_child
        return 1
    fi
    sleep "$idle_seconds"
    for _ in 1 2 3 4 5; do
        value=$(rss_kib "$active_pid")
        [[ -n $value ]] && samples+=("$value")
        sleep 0.1
    done
    cleanup_child
    ((${#samples[@]} > 0)) || {
        printf 'not measured (`VmRSS` unavailable)'
        return 1
    }
    printf '%s KiB (median of %s samples, Qt offscreen platform)' \
        "$(median "${samples[@]}")" "${#samples[@]}"
}

measure_gui_runtime() {
    local -a startup_ms=()
    local -a rss_samples=()
    local run start_ns now_ns deadline_ns mapped value
    for ((run = 1; run <= runs; run++)); do
        start_ns=$(date +%s%N)
        deadline_ns=$((start_ns + 10000000000))
        mapped=false
        start_native gui
        while kill -0 "$active_pid" 2>/dev/null; do
            if hyprctl -j clients 2>/dev/null \
                | jq -e --argjson pid "$active_pid" 'any(.[]; .pid == $pid)' \
                    >/dev/null 2>&1; then
                now_ns=$(date +%s%N)
                startup_ms+=("$(( (now_ns - start_ns) / 1_000_000 ))")
                mapped=true
                break
            fi
            now_ns=$(date +%s%N)
            ((now_ns < deadline_ns)) || break
            sleep 0.01
        done
        if [[ $mapped != true ]]; then
            cleanup_child
            return 1
        fi
        sleep "$idle_seconds"
        value=$(rss_kib "$active_pid")
        [[ -n $value ]] && rss_samples+=("$value")
        cleanup_child
    done

    gui_first_ms=${startup_ms[0]}
    if ((${#startup_ms[@]} > 1)); then
        gui_warm_median_ms=$(median "${startup_ms[@]:1}")
    else
        gui_warm_median_ms='not measured (one run)'
    fi
    if ((${#rss_samples[@]} > 0)); then
        gui_rss_kib=$(median "${rss_samples[@]}")
    else
        gui_rss_kib='not measured'
    fi
}

native_bytes=$(stat -c '%s' -- "$native_binary")
tauri_present=false
if [[ -f $tauri_binary && -x $tauri_binary ]]; then
    tauri_present=true
    tauri_bytes=$(stat -c '%s' -- "$tauri_binary")
fi

printf '# Dicta native benchmark report\n\n'
printf -- '- Generated: `%s`\n' "$(date --iso-8601=seconds)"
printf -- '- Kernel: `%s`\n' "$(uname -srmo)"
printf -- '- Architecture: `%s`\n' "$(uname -m)"
if command -v qmake6 >/dev/null 2>&1; then
    qt_version=$(qmake6 -query QT_VERSION 2>/dev/null || printf 'unknown')
else
    qt_version=unknown
fi
printf -- '- Qt: `%s`\n' "$qt_version"
printf -- '- gpu-screen-recorder: `%s`\n' \
    "$(gpu-screen-recorder --version 2>/dev/null || printf 'not installed')"
printf '\n## Static artifact measurements\n\n'
emit_binary_facts 'Native Qt' "$native_binary"

if $tauri_present; then
    emit_binary_facts 'Existing Tauri artifact' "$tauri_binary"
    awk -v native="$native_bytes" -v tauri="$tauri_bytes" 'BEGIN {
        printf "The existing Tauri artifact is %.2fx the native artifact size; the native artifact is %.1f%% smaller.\n\n", tauri / native, (1 - native / tauri) * 100
    }'
else
    printf '### Existing Tauri artifact\n\nNot compared: `%s` is not present and this script never builds or downloads it.\n\n' "$tauri_binary"
fi

printf '## Runtime measurements\n\n'
if [[ $runtime_mode == auto ]]; then
    if hyprland_probe_available; then
        runtime_mode=gui
    else
        runtime_mode=headless
    fi
fi

case $runtime_mode in
    gui)
        if ! hyprland_probe_available; then
            printf -- '- Startup to mapped window: not measured (`hyprctl`/`jq` cannot query this session).\n'
            printf -- '- Idle RSS: not measured in GUI mode. Re-run with `--runtime headless` for an offscreen proxy.\n'
        elif measure_gui_runtime; then
            printf -- '- First startup to mapped window: %s ms. This is not a true cold-cache measurement; OS page caches were not dropped.\n' "$gui_first_ms"
            printf -- '- Subsequent startup median to mapped window: %s ms (%s total runs).\n' "$gui_warm_median_ms" "$runs"
            printf -- '- Idle RSS: %s KiB median after %s seconds.\n' "$gui_rss_kib" "$idle_seconds"
        else
            printf -- '- Startup to mapped window: not measured (the process exited or no matching Hyprland window mapped within 10 seconds).\n'
            printf -- '- Idle RSS: not measured because GUI startup did not reach the mapping proxy.\n'
        fi
        ;;
    headless)
        printf -- '- Startup to event loop: not measured. Qt offscreen mode exposes no reliable event-loop-ready marker.\n'
        printf -- '- Idle RSS: %s.\n' "$(measure_headless_rss || true)"
        ;;
    skip)
        printf -- '- Startup and idle RSS: skipped by request.\n'
        ;;
esac

printf '\n## Method notes\n\n'
printf '%s\n' \
    '- File size is the exact current ELF artifact size; neither artifact is copied or stripped.' \
    '- Direct libraries come from ELF `DT_NEEDED`; the `ldd` count includes transitively resolved runtime entries.' \
    '- GUI startup uses the first Hyprland client entry for the child PID as a repeatable event-loop/window-map proxy.' \
    '- “First” and “subsequent” runs do not claim physical cold-cache timing because this script does not require root or drop Linux page caches.' \
    '- Tauri is inspected only when an executable already exists. It is not launched, rebuilt, or allowed to mutate application state.'

if [[ -s $runtime_log ]]; then
    printf '\n## Runtime diagnostics\n\n```text\n'
    sed -n '1,40p' "$runtime_log"
    printf '```\n'
fi
