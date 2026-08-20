#!/usr/bin/env bash
set -eu

native_binary=$1
cli_binary=$2
fixture=$(mktemp -d -t dicta-native-process-e2e.XXXXXXXX)

cleanup() {
    rm -rf -- "$fixture"
}
trap cleanup EXIT INT TERM

mkdir -m 700 "$fixture/runtime"
export XDG_RUNTIME_DIR="$fixture/runtime"
export DICTA_SOCKET="$fixture/runtime/dicta/control-v1.sock"
export DICTA_STORAGE_ROOT="$fixture/storage"
export DICTA_NATIVE_BIN="$native_binary"
export DICTA_NATIVE_E2E=1
export DICTA_NATIVE_E2E_EXIT_AFTER_STOP=1
export DICTA_NATIVE_E2E_MAX_MS=10000
export DICTA_NATIVE_E2E_UI_MARKER="$fixture/ui-lifecycle"
export DICTA_NATIVE_E2E_HIDE_UI_AFTER_MS=75
export QT_QPA_PLATFORM=offscreen
export QT_QPA_PLATFORMTHEME=
export QT_STYLE_OVERRIDE=

version=$($native_binary --version)
case "$version" in
    *'0.8.0'*) ;;
    *) echo "expected native version 0.8.0, received: $version" >&2; exit 1 ;;
esac

$cli_binary ui >/dev/null
attempt=0
while ! grep -q '^hidden$' "$DICTA_NATIVE_E2E_UI_MARKER" 2>/dev/null \
    && [ "$attempt" -lt 100 ]; do
    sleep 0.05
    attempt=$((attempt + 1))
done
grep -q '^shown$' "$DICTA_NATIVE_E2E_UI_MARKER"
grep -q '^hidden$' "$DICTA_NATIVE_E2E_UI_MARKER"
socket_inode=$(stat -c %i "$DICTA_SOCKET")
$cli_binary ui >/dev/null
test "$(stat -c %i "$DICTA_SOCKET")" = "$socket_inode"

idle=$($cli_binary --json status)
case "$idle" in
    *'"phase":"idle"'*) ;;
    *) echo "expected idle status, received: $idle" >&2; exit 1 ;;
esac

$cli_binary record start --note "native process E2E" >/dev/null
recording=$($cli_binary --json status)
case "$recording" in
    *'"phase":"recording"'*) ;;
    *) echo "expected recording status, received: $recording" >&2; exit 1 ;;
esac

$cli_binary record stop >/dev/null
attempt=0
while [ -e "$DICTA_SOCKET" ] && [ "$attempt" -lt 100 ]; do
    sleep 0.05
    attempt=$((attempt + 1))
done

if [ -e "$DICTA_SOCKET" ]; then
    echo "dicta-native did not stop and remove its socket" >&2
    exit 1
fi

test -f "$DICTA_STORAGE_ROOT/e2e/e2e-000001.json"
test -f "$DICTA_STORAGE_ROOT/e2e/e2e-000001.annotations.json"
