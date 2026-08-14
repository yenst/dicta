#!/usr/bin/env bash
set -euo pipefail

project_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
bundle_root="$project_root/src-tauri/target/release/bundle/linux"
stage_root="$bundle_root/Dicta"
archive_path="$bundle_root/Dicta_0.8.0_linux_x86_64.tar.gz"

cd "$project_root"
npm run tauri build -- --no-bundle

install -Dm755 src-tauri/target/release/dicta "$stage_root/usr/bin/dicta"
install -Dm755 mcp/target/release/dicta-mcp "$stage_root/usr/lib/Dicta/dicta-mcp"
install -Dm644 src-tauri/resources/ggml-base-q5_1.bin "$stage_root/usr/lib/Dicta/ggml-base-q5_1.bin"
install -Dm644 src-tauri/dicta.desktop "$stage_root/usr/share/applications/dicta.desktop"
install -Dm644 src-tauri/icons/32x32.png "$stage_root/usr/share/icons/hicolor/32x32/apps/dicta.png"
install -Dm644 src-tauri/icons/128x128.png "$stage_root/usr/share/icons/hicolor/128x128/apps/dicta.png"
install -Dm644 src-tauri/icons/128x128@2x.png "$stage_root/usr/share/icons/hicolor/256x256/apps/dicta.png"
install -Dm644 src-tauri/icons/icon.png "$stage_root/usr/share/icons/hicolor/512x512/apps/dicta.png"

tar -czf "$archive_path" -C "$bundle_root" Dicta
echo "Linux bundle: $archive_path"
