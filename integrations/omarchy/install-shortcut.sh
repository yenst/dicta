#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'EOF'
Usage: dicta-install-omarchy-shortcut [--shortcut ID] [--remove] [--no-reload]

Installs or removes Dicta's isolated Hyprland binding module. Supported IDs:
  alt_shift_r, command_shift_d, option_space, control_space

Without --shortcut, the persisted Dicta setting is used when available.
--no-reload is intended only for packaging and isolated verification.
EOF
}

shortcut_id=""
remove=0
reload=1
while (($#)); do
  case "$1" in
    --shortcut)
      [[ $# -ge 2 ]] || { echo "--shortcut requires an ID" >&2; exit 64; }
      shortcut_id="$2"
      shift 2
      ;;
    --remove)
      remove=1
      shift
      ;;
    --no-reload)
      reload=0
      shift
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      echo "Unknown argument: $1" >&2
      usage >&2
      exit 64
      ;;
  esac
done

config_home="${XDG_CONFIG_HOME:-${HOME:?HOME is required}/.config}"
hypr_dir="$config_home/hypr"
bindings="$hypr_dir/bindings.lua"
managed="$hypr_dir/dicta-bindings.lua"
require_line='require("hypr.dicta-bindings")'
require_comment='-- Dicta managed shortcut integration.'

reject_unsafe() {
  local path="$1"
  local label="$2"
  if [[ -L "$path" ]]; then
    echo "$label cannot be a symlink: $path" >&2
    exit 77
  fi
}

reject_unsafe "$hypr_dir" "Hyprland configuration directory"
[[ -d "$hypr_dir" ]] || { echo "Missing Hyprland configuration: $hypr_dir" >&2; exit 66; }
reject_unsafe "$bindings" "Hyprland bindings"
[[ -f "$bindings" ]] || { echo "Missing Hyprland bindings: $bindings" >&2; exit 66; }
reject_unsafe "$managed" "Managed Dicta shortcut"

temporary="$(mktemp "$hypr_dir/.dicta-shortcut.XXXXXX")"
bindings_backup="$(mktemp "$hypr_dir/.dicta-bindings-backup.XXXXXX")"
managed_backup="$(mktemp "$hypr_dir/.dicta-module-backup.XXXXXX")"
persistent_backup="$(mktemp "$bindings.dicta-backup.XXXXXX")"
managed_existed=0
cp -p -- "$bindings" "$bindings_backup"
cp -p -- "$bindings" "$persistent_backup"
if [[ -f "$managed" ]]; then
  cp -p -- "$managed" "$managed_backup"
  managed_existed=1
fi
cleanup() {
  rm -f -- "$temporary" "$bindings_backup" "$managed_backup"
}
trap cleanup EXIT INT TERM

restore_previous() {
  cp -p -- "$bindings_backup" "$bindings"
  if ((managed_existed)); then
    cp -p -- "$managed_backup" "$managed"
  else
    rm -f -- "$managed"
  fi
}

validate_reload() {
  ((reload)) || return 0
  command -v hyprctl >/dev/null 2>&1 || {
    echo "hyprctl is required to activate and validate the shortcut" >&2
    return 1
  }
  hyprctl reload >/dev/null || return 1
  local errors
  errors="$(hyprctl configerrors 2>&1)" || return 1
  if [[ -n "${errors//[[:space:]]/}" ]]; then
    echo "$errors" >&2
    return 1
  fi
}

if ((remove)); then
  awk -v require_line="$require_line" -v require_comment="$require_comment" '
    $0 == require_line || $0 == require_comment { next }
    { print }
  ' "$bindings" >"$temporary"
  chmod --reference="$bindings" "$temporary"
  mv -f -- "$temporary" "$bindings"
  rm -f -- "$managed"
  if ! validate_reload; then
    echo "Hyprland rejected the removal; restoring the previous Dicta integration." >&2
    restore_previous
    if ((reload)); then hyprctl reload >/dev/null 2>&1 || true; fi
    exit 78
  fi
  echo "Removed Dicta's managed Omarchy shortcut. Previous bindings are active again."
  echo "Bindings backup: $persistent_backup"
  exit 0
fi

if [[ -z "$shortcut_id" ]]; then
  storage_root="${DICTA_STORAGE_ROOT:-${DICTA_HOME:-}}"
  if [[ -z "$storage_root" ]]; then
    if [[ -d "$HOME/Documents/Dicta" ]]; then
      storage_root="$HOME/Documents/Dicta"
    else
      storage_root="$HOME/Documents/PromptReel"
    fi
  fi
  settings_file="$storage_root/settings.json"
  if [[ -f "$settings_file" && ! -L "$settings_file" ]] && command -v jq >/dev/null 2>&1; then
    shortcut_id="$(jq -r '.shortcut_id // empty' "$settings_file" 2>/dev/null || true)"
  fi
fi
shortcut_id="${shortcut_id:-alt_shift_r}"

case "$shortcut_id" in
  command_shift_r|alt_shift_r)
    shortcut_id="alt_shift_r"
    sequence="ALT + SHIFT + R"
    modmask=9
    key="R"
    ;;
  command_shift_d)
    sequence="SUPER + SHIFT + D"
    modmask=65
    key="D"
    ;;
  option_space)
    sequence="ALT + SPACE"
    modmask=8
    key="SPACE"
    ;;
  control_space)
    sequence="CTRL + SPACE"
    modmask=4
    key="SPACE"
    ;;
  *)
    echo "Unknown Dicta shortcut preset: $shortcut_id" >&2
    exit 64
    ;;
esac

previous_binding=""
if ((reload)) && command -v hyprctl >/dev/null 2>&1 && command -v jq >/dev/null 2>&1; then
  previous_binding="$(hyprctl binds -j 2>/dev/null | jq -r \
    --argjson mask "$modmask" --arg key "$key" \
    '[.[] | select(.modmask == $mask and ((.key // "") | ascii_upcase) == $key) | (.description // .arg // .dispatcher // "unnamed action")][0] // empty' \
    2>/dev/null || true)"
fi

legacy_migrated=0
awk -v require_line="$require_line" -v require_comment="$require_comment" '
  /o\.bind\(.*Toggle Dicta recording.*dicta --toggle-recording.*\)/ { legacy = 1; next }
  $0 == require_line || $0 == require_comment { next }
  { print }
  END {
    print ""
    print require_comment
    print require_line
    if (legacy) exit 42
  }
' "$bindings" >"$temporary" || {
  status=$?
  if [[ $status -eq 42 ]]; then legacy_migrated=1; else exit "$status"; fi
}
chmod --reference="$bindings" "$temporary"
mv -f -- "$temporary" "$bindings"

temporary="$(mktemp "$hypr_dir/.dicta-shortcut.XXXXXX")"
printf '%s\n' \
  '-- Managed by Dicta. Re-run dicta-install-omarchy-shortcut to repair or remove it.' \
  '-- The selected key is released from its previous action before Dicta claims it.' \
  "hl.unbind(\"$sequence\")" \
  "o.bind(\"$sequence\", \"Toggle Dicta recording\", \"dicta record toggle\")" \
  >"$temporary"
chmod 0644 "$temporary"
mv -f -- "$temporary" "$managed"

if ! validate_reload; then
  echo "Hyprland rejected the Dicta shortcut; restoring the previous configuration." >&2
  restore_previous
  if ((reload)); then hyprctl reload >/dev/null 2>&1 || true; fi
  exit 78
fi

echo "Installed Dicta's Omarchy shortcut: $sequence → dicta record toggle"
echo "Bindings backup: $persistent_backup"
if [[ -n "$previous_binding" && "$previous_binding" != "Toggle Dicta recording" ]]; then
  echo "Note: $sequence was previously bound to: $previous_binding"
  echo "The managed module unbinds that action while Dicta owns this preset."
fi
if ((legacy_migrated)); then
  echo "Migrated the obsolete dicta --toggle-recording binding."
fi
