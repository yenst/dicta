#!/usr/bin/env bash
set -euo pipefail

project_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$project_root"

for command_name in cargo cmake find grep gzip install node readelf rustc tar; do
  if ! command -v "$command_name" >/dev/null 2>&1; then
    echo "Missing required native packaging command: $command_name" >&2
    exit 1
  fi
done

version="$(node scripts/check-versions.mjs --print)"
if [[ ! "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.-]+)?(\+[0-9A-Za-z.-]+)?$ ]]; then
  echo "Invalid package version: $version" >&2
  exit 1
fi
node integrations/omarchy/dicta-context/tests/native-cli-contract.mjs
bash integrations/omarchy/tests/shortcut-install-test.sh
if ! grep -Fxq 'Exec=dicta ui' apps/dicta-native/dicta-native.desktop; then
  echo "The desktop launcher must activate the single native host through 'dicta ui'" >&2
  exit 1
fi

host_triple="$(rustc -vV | sed -n 's/^host: //p')"
case "$host_triple" in
  x86_64-*-linux-*) archive_arch="x86_64" ;;
  aarch64-*-linux-*) archive_arch="aarch64" ;;
  *-linux-*) archive_arch="${host_triple%%-*}" ;;
  *)
    echo "The native Linux archive must be built on Linux, not $host_triple" >&2
    exit 1
    ;;
esac
if [[ -n "${CARGO_BUILD_TARGET:-}" && "$CARGO_BUILD_TARGET" != "$host_triple" ]]; then
  echo "Cross-compiling the Qt host is not supported; build on $CARGO_BUILD_TARGET instead" >&2
  exit 1
fi

if [[ -n "${CARGO_TARGET_DIR:-}" ]]; then
  cargo_target_root="$CARGO_TARGET_DIR"
  if [[ "$cargo_target_root" != /* ]]; then
    cargo_target_root="$project_root/$cargo_target_root"
  fi
  mcp_binary="$cargo_target_root/release/dicta-mcp"
else
  cargo_target_root="$project_root/target"
  mcp_binary="$project_root/mcp/target/release/dicta-mcp"
fi

native_build_dir="$cargo_target_root/native-cmake-release"
bundle_root="$cargo_target_root/bundle/native-linux"
archive_name="Dicta_${version}_native_linux_${archive_arch}.tar.gz"
archive_path="$bundle_root/$archive_name"

mkdir -p "$bundle_root"
stage_workspace="$(mktemp -d "$bundle_root/.dicta-native-stage.XXXXXX")"
archive_tmp="$(mktemp "$bundle_root/.${archive_name}.XXXXXX")"
cleanup() {
  rm -rf -- "$stage_workspace"
  rm -f -- "$archive_tmp"
}
trap cleanup EXIT INT TERM
stage_root="$stage_workspace/Dicta"

cargo build --release --locked --package dicta-cli
cargo build --release --locked --manifest-path mcp/Cargo.toml
cmake \
  -S apps/dicta-native \
  -B "$native_build_dir" \
  -DCMAKE_BUILD_TYPE=Release \
  -DDICTA_BUILD_TESTS=OFF
cmake --build "$native_build_dir" --parallel
cmake --install "$native_build_dir" --prefix "$stage_root/usr"

install -Dm755 "$cargo_target_root/release/dicta" "$stage_root/usr/bin/dicta"
install -Dm755 "$mcp_binary" "$stage_root/usr/lib/Dicta/dicta-mcp"
install -Dm644 src-tauri/resources/ggml-base-q5_1.bin "$stage_root/usr/lib/Dicta/ggml-base-q5_1.bin"
install -Dm644 apps/dicta-native/dicta-native.desktop "$stage_root/usr/share/applications/dicta.desktop"
install -Dm644 apps/dicta-native/README.md "$stage_root/usr/share/doc/Dicta/README.md"
install -Dm644 src-tauri/icons/32x32.png "$stage_root/usr/share/icons/hicolor/32x32/apps/dicta.png"
install -Dm644 src-tauri/icons/128x128.png "$stage_root/usr/share/icons/hicolor/128x128/apps/dicta.png"
install -Dm644 src-tauri/icons/128x128@2x.png "$stage_root/usr/share/icons/hicolor/256x256/apps/dicta.png"
install -Dm644 src-tauri/icons/icon.png "$stage_root/usr/share/icons/hicolor/512x512/apps/dicta.png"
install -Dm755 integrations/omarchy/install.sh "$stage_root/usr/bin/dicta-install-omarchy-plugin"
install -Dm755 integrations/omarchy/install-shortcut.sh "$stage_root/usr/bin/dicta-install-omarchy-shortcut"
install -Dm644 integrations/omarchy/dicta-context/manifest.json "$stage_root/usr/share/Dicta/omarchy/dicta.context/manifest.json"
install -Dm644 integrations/omarchy/dicta-context/Panel.qml "$stage_root/usr/share/Dicta/omarchy/dicta.context/Panel.qml"
install -Dm644 integrations/omarchy/dicta-context/Service.qml "$stage_root/usr/share/Dicta/omarchy/dicta.context/Service.qml"
install -Dm644 integrations/omarchy/dicta-context/README.md "$stage_root/usr/share/Dicta/omarchy/dicta.context/README.md"
install -Dm644 integrations/omarchy/dicta-context/assets/dicta-mark-light.png "$stage_root/usr/share/Dicta/omarchy/dicta.context/assets/dicta-mark-light.png"

test -x "$stage_root/usr/bin/dicta"
test -x "$stage_root/usr/bin/dicta-native"
test -x "$stage_root/usr/bin/dicta-install-omarchy-plugin"
test -x "$stage_root/usr/bin/dicta-install-omarchy-shortcut"
test -x "$stage_root/usr/lib/Dicta/dicta-mcp"
test -s "$stage_root/usr/lib/Dicta/ggml-base-q5_1.bin"

if find "$stage_root/usr" -type f \( -name '*.so' -o -name '*.so.*' \) -print -quit \
  | grep -q .; then
  echo "The native archive must use system libraries instead of bundling shared objects" >&2
  exit 1
fi
for executable in \
  "$stage_root/usr/bin/dicta" \
  "$stage_root/usr/bin/dicta-native" \
  "$stage_root/usr/lib/Dicta/dicta-mcp"; do
  if readelf -d "$executable" 2>/dev/null \
    | grep -Eiq 'webkit|javascriptcore|node'; then
    echo "Legacy web/runtime dependency detected in $executable" >&2
    exit 1
  fi
done

source_date_epoch="${SOURCE_DATE_EPOCH:-$(git log -1 --format=%ct 2>/dev/null || printf '0')}"
if [[ ! "$source_date_epoch" =~ ^[0-9]+$ ]]; then
  echo "SOURCE_DATE_EPOCH must be an integer, not $source_date_epoch" >&2
  exit 1
fi
tar \
  --sort=name \
  --mtime="@$source_date_epoch" \
  --owner=0 \
  --group=0 \
  --numeric-owner \
  -cf - \
  -C "$stage_workspace" \
  Dicta | gzip -n >"$archive_tmp"

archive_listing="$stage_workspace/archive.list"
tar -tzf "$archive_tmp" >"$archive_listing"
for required_path in \
  Dicta/usr/bin/dicta \
  Dicta/usr/bin/dicta-native \
  Dicta/usr/bin/dicta-install-omarchy-plugin \
  Dicta/usr/bin/dicta-install-omarchy-shortcut \
  Dicta/usr/lib/Dicta/dicta-mcp \
  Dicta/usr/lib/Dicta/ggml-base-q5_1.bin \
  Dicta/usr/share/applications/dicta.desktop \
  Dicta/usr/share/doc/Dicta/README.md \
  Dicta/usr/share/icons/hicolor/512x512/apps/dicta.png \
  Dicta/usr/share/Dicta/omarchy/dicta.context/manifest.json \
  Dicta/usr/share/Dicta/omarchy/dicta.context/Panel.qml \
  Dicta/usr/share/Dicta/omarchy/dicta.context/Service.qml \
  Dicta/usr/share/Dicta/omarchy/dicta.context/README.md; do
  if ! grep -Fxq "$required_path" "$archive_listing"; then
    echo "Native Linux archive is missing $required_path" >&2
    exit 1
  fi
done

verification_root="$stage_workspace/verify"
mkdir "$verification_root"
tar -xzf "$archive_tmp" -C "$verification_root"
packaged_root="$verification_root/Dicta"
"$packaged_root/usr/bin/dicta" --help >/dev/null
QT_QPA_PLATFORM=offscreen \
QT_QPA_PLATFORMTHEME= \
QT_STYLE_OVERRIDE= \
  "$packaged_root/usr/bin/dicta-native" --smoke-overlay
node scripts/smoke-mcp.mjs "$packaged_root/usr/lib/Dicta/dicta-mcp"

mv -f -- "$archive_tmp" "$archive_path"
chmod 0644 "$archive_path"
echo "Native Linux bundle: $archive_path"
