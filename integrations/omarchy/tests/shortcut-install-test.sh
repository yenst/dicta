#!/usr/bin/env bash
set -euo pipefail

project_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
fixture="$(mktemp -d /tmp/dicta-shortcut-test.XXXXXX)"
cleanup() { rm -rf -- "$fixture"; }
trap cleanup EXIT INT TERM

mkdir -p "$fixture/config/hypr" "$fixture/storage"
printf '%s\n' \
  '-- user bindings' \
  'o.bind("CTRL + SPACE", "Toggle Dicta recording", "/home/test/.local/bin/dicta --toggle-recording")' \
  >"$fixture/config/hypr/bindings.lua"
printf '%s\n' '{"shortcut_id":"control_space"}' >"$fixture/storage/settings.json"

XDG_CONFIG_HOME="$fixture/config" \
DICTA_HOME="$fixture/storage" \
  "$project_root/integrations/omarchy/install-shortcut.sh" --no-reload

grep -Fxq 'require("hypr.dicta-bindings")' "$fixture/config/hypr/bindings.lua"
! grep -Fq -- '--toggle-recording' "$fixture/config/hypr/bindings.lua"
grep -Fxq 'hl.unbind("CTRL + SPACE")' "$fixture/config/hypr/dicta-bindings.lua"
grep -Fxq 'o.bind("CTRL + SPACE", "Toggle Dicta recording", "dicta record toggle")' \
  "$fixture/config/hypr/dicta-bindings.lua"
grep -Fxq 'hl.unbind("SUPER + ALT + A")' "$fixture/config/hypr/dicta-bindings.lua"
grep -Fxq 'hl.unbind("F8")' "$fixture/config/hypr/dicta-bindings.lua"
grep -Fxq 'o.bind("F8", "Draw Dicta annotation (hold)", "dicta annotate enable")' \
  "$fixture/config/hypr/dicta-bindings.lua"
grep -Fxq 'o.bind("F8", nil, "dicta annotate disable", { release = true })' \
  "$fixture/config/hypr/dicta-bindings.lua"
grep -Fq 'title = "^Dicta Annotation Overlay$"' "$fixture/config/hypr/dicta-bindings.lua"
! grep -Fq 'title = "^Dicta Annotation Helper$"' "$fixture/config/hypr/dicta-bindings.lua"
grep -Fq 'title = "^Dicta status$"' "$fixture/config/hypr/dicta-bindings.lua"
grep -Fq 'float = true' "$fixture/config/hypr/dicta-bindings.lua"
grep -Fq 'monitor_h-window_h-34' "$fixture/config/hypr/dicta-bindings.lua"
[[ "$(find "$fixture/config/hypr" -maxdepth 1 -type f -name 'bindings.lua.dicta-backup.*' | wc -l)" -ge 1 ]]

XDG_CONFIG_HOME="$fixture/config" \
  "$project_root/integrations/omarchy/install-shortcut.sh" \
  --shortcut command_shift_d --no-reload
grep -Fxq 'hl.unbind("SUPER + SHIFT + D")' "$fixture/config/hypr/dicta-bindings.lua"
[[ "$(grep -Fxc 'require("hypr.dicta-bindings")' "$fixture/config/hypr/bindings.lua")" -eq 1 ]]

XDG_CONFIG_HOME="$fixture/config" \
  "$project_root/integrations/omarchy/install-shortcut.sh" --remove --no-reload
! grep -Fq 'hypr.dicta-bindings' "$fixture/config/hypr/bindings.lua"
[[ ! -e "$fixture/config/hypr/dicta-bindings.lua" ]]

printf '%s\n' sentinel >"$fixture/outside.lua"
ln -s "$fixture/outside.lua" "$fixture/config/hypr/dicta-bindings.lua"
if XDG_CONFIG_HOME="$fixture/config" \
  "$project_root/integrations/omarchy/install-shortcut.sh" --no-reload; then
  echo "symlinked managed module was unexpectedly accepted" >&2
  exit 1
fi
grep -Fxq sentinel "$fixture/outside.lua"

echo "Dicta Omarchy shortcut installer: ok"
