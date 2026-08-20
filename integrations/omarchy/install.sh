#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
checkout_source="$script_dir/dicta-context"
packaged_source="/usr/share/Dicta/omarchy/dicta.context"
if [[ -f "$checkout_source/manifest.json" ]]; then
  source_dir="$checkout_source"
elif [[ -f "$packaged_source/manifest.json" ]]; then
  source_dir="$packaged_source"
else
  echo "Could not locate the packaged Dicta Omarchy plugin." >&2
  exit 1
fi

destination="${XDG_CONFIG_HOME:-$HOME/.config}/omarchy/plugins/dicta.context"
if [[ -e "$destination" ]]; then
  echo "Refusing to overwrite existing plugin: $destination" >&2
  echo "Remove it with 'omarchy plugin remove dicta.context' before reinstalling." >&2
  exit 1
fi

omarchy plugin validate "$source_dir"
install -d "$(dirname "$destination")"
cp -a "$source_dir" "$destination"
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

echo "Installed Dicta context plugin: $destination"
