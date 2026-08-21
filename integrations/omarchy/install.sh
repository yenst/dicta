#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
checkout_source="$script_dir/dicta-context"
user_packaged_source="${XDG_DATA_HOME:-$HOME/.local/share}/Dicta/omarchy/dicta.context"
packaged_source="/usr/share/Dicta/omarchy/dicta.context"
if [[ -f "$checkout_source/manifest.json" ]]; then
  source_dir="$checkout_source"
elif [[ -f "$user_packaged_source/manifest.json" ]]; then
  source_dir="$user_packaged_source"
elif [[ -f "$packaged_source/manifest.json" ]]; then
  source_dir="$packaged_source"
else
  echo "Could not locate the packaged Dicta Omarchy plugin." >&2
  exit 1
fi

destination="${XDG_CONFIG_HOME:-$HOME/.config}/omarchy/plugins/dicta.context"
backup_directory="${XDG_STATE_HOME:-$HOME/.local/state}/dicta/omarchy-plugin-backups"
omarchy plugin validate "$source_dir"
install -d "$(dirname "$destination")"
install -d "$backup_directory"

# Omarchy watches every directory below plugins/. Keeping rollback copies next
# to the live plugin makes the shell discover duplicate manifests and hot-reload
# them repeatedly. Move backups created by older Dicta installers out of the
# watched tree before replacing the live copy.
shopt -s nullglob
for legacy_backup in "${destination}.backup."* "${destination}.failed."*; do
  mv "$legacy_backup" "$backup_directory/$(basename "$legacy_backup")"
done
shopt -u nullglob

backup=""
if [[ -L "$destination" ]]; then
  echo "Refusing to replace symlinked plugin: $destination" >&2
  exit 1
elif [[ -e "$destination" ]]; then
  [[ -d "$destination" ]] || {
    echo "Refusing to replace non-directory plugin: $destination" >&2
    exit 1
  }
  backup="$backup_directory/dicta.context.backup.$(date +%Y%m%d-%H%M%S)"
  mv "$destination" "$backup"
fi
if ! cp -a "$source_dir" "$destination"; then
  failed="$backup_directory/dicta.context.failed.$(date +%Y%m%d-%H%M%S)"
  [[ ! -e "$destination" ]] || mv "$destination" "$failed"
  [[ -z "$backup" ]] || mv "$backup" "$destination"
  exit 1
fi
omarchy-shell shell rescanPlugins >/dev/null
discovered=0
for ((attempt = 0; attempt < 40; attempt++)); do
  if omarchy plugin list --json | jq -e --arg id "dicta.context" \
    'any(.[]; .id == $id)' >/dev/null; then
    discovered=1
    break
  fi
  sleep 0.05
done
if (( ! discovered )); then
  echo "Dicta context was copied but Omarchy did not discover it." >&2
  exit 1
fi
omarchy plugin enable dicta.context

if [[ -n "$backup" ]]; then
  echo "Updated Dicta context plugin: $destination"
  echo "Previous plugin backup: $backup"
else
  echo "Installed Dicta context plugin: $destination"
fi
