#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
asset_root="$repo_root/apps/dicta-native/assets"
icon_root="$repo_root/apps/dicta-native/icons"
integration_mark="$repo_root/integrations/omarchy/dicta-context/assets/dicta-mark-light.png"
master="$asset_root/dicta-mark.png"

if ! command -v magick >/dev/null 2>&1; then
  echo "ImageMagick is required to regenerate Dicta icons" >&2
  exit 1
fi

test -s "$master"
cp "$master" "$asset_root/dicta-mark-light.png"
cp "$master" "$integration_mark"

for size in 32 128 256 512; do
  magick "$master" -filter Lanczos -resize "${size}x${size}" -strip \
    "PNG32:$icon_root/${size}x${size}.png"
done
