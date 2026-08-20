#!/usr/bin/env bash
set -euo pipefail

project_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$project_root"

for command_name in cargo gzip install node npm rustc tar; do
  if ! command -v "$command_name" >/dev/null 2>&1; then
    echo "Missing required packaging command: $command_name" >&2
    exit 1
  fi
done

version="$(node scripts/check-versions.mjs --print)"
if [[ ! "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.-]+)?(\+[0-9A-Za-z.-]+)?$ ]]; then
  echo "Invalid package version: $version" >&2
  exit 1
fi

host_triple="$(rustc -vV | sed -n 's/^host: //p')"
target_triple="${CARGO_BUILD_TARGET:-$host_triple}"
case "$target_triple" in
  *-linux-*) ;;
  *)
    echo "The Linux archive must be built for a Linux target, not $target_triple" >&2
    exit 1
    ;;
esac
case "$target_triple" in
  x86_64-*) archive_arch="x86_64" ;;
  aarch64-*) archive_arch="aarch64" ;;
  *) archive_arch="${target_triple%%-*}" ;;
esac

if [[ -n "${CARGO_TARGET_DIR:-}" ]]; then
  cargo_target_root="$CARGO_TARGET_DIR"
  if [[ "$cargo_target_root" != /* ]]; then
    cargo_target_root="$project_root/$cargo_target_root"
  fi
  app_target_root="$cargo_target_root"
  mcp_target_root="$cargo_target_root"
else
  app_target_root="$project_root/src-tauri/target"
  mcp_target_root="$project_root/mcp/target"
fi

release_suffix="release"
if [[ -n "${CARGO_BUILD_TARGET:-}" ]]; then
  release_suffix="$target_triple/release"
fi
app_release_dir="$app_target_root/$release_suffix"
mcp_release_dir="$mcp_target_root/$release_suffix"
bundle_root="$app_release_dir/bundle/linux"
archive_name="Dicta_${version}_linux_${archive_arch}.tar.gz"
archive_path="$bundle_root/$archive_name"

mkdir -p "$bundle_root"
stage_workspace="$(mktemp -d "$bundle_root/.dicta-stage.XXXXXX")"
archive_tmp="$(mktemp "$bundle_root/.${archive_name}.XXXXXX")"
cleanup() {
  rm -rf -- "$stage_workspace"
  rm -f -- "$archive_tmp"
}
trap cleanup EXIT
stage_root="$stage_workspace/Dicta"

cargo build --release --locked --manifest-path mcp/Cargo.toml
npm run tauri -- build --no-bundle -- --locked

install -Dm755 "$app_release_dir/dicta" "$stage_root/usr/bin/dicta"
install -Dm755 "$mcp_release_dir/dicta-mcp" "$stage_root/usr/lib/Dicta/dicta-mcp"
install -Dm644 src-tauri/resources/ggml-base-q5_1.bin "$stage_root/usr/lib/Dicta/ggml-base-q5_1.bin"
install -Dm644 src-tauri/dicta.desktop "$stage_root/usr/share/applications/dicta.desktop"
install -Dm644 src-tauri/icons/32x32.png "$stage_root/usr/share/icons/hicolor/32x32/apps/dicta.png"
install -Dm644 src-tauri/icons/128x128.png "$stage_root/usr/share/icons/hicolor/128x128/apps/dicta.png"
install -Dm644 src-tauri/icons/128x128@2x.png "$stage_root/usr/share/icons/hicolor/256x256/apps/dicta.png"
install -Dm644 src-tauri/icons/icon.png "$stage_root/usr/share/icons/hicolor/512x512/apps/dicta.png"

test -x "$stage_root/usr/bin/dicta"
test -x "$stage_root/usr/lib/Dicta/dicta-mcp"
test -s "$stage_root/usr/lib/Dicta/ggml-base-q5_1.bin"

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
  Dicta/usr/lib/Dicta/dicta-mcp \
  Dicta/usr/lib/Dicta/ggml-base-q5_1.bin \
  Dicta/usr/share/applications/dicta.desktop \
  Dicta/usr/share/icons/hicolor/512x512/apps/dicta.png; do
  if ! grep -Fxq "$required_path" "$archive_listing"; then
    echo "Linux archive is missing $required_path" >&2
    exit 1
  fi
done

verification_root="$stage_workspace/verify"
mkdir "$verification_root"
tar -xzf "$archive_tmp" -C "$verification_root"
test -x "$verification_root/Dicta/usr/bin/dicta"
test -x "$verification_root/Dicta/usr/lib/Dicta/dicta-mcp"
test -s "$verification_root/Dicta/usr/lib/Dicta/ggml-base-q5_1.bin"
if [[ "$target_triple" == "$host_triple" ]]; then
  node scripts/smoke-mcp.mjs "$verification_root/Dicta/usr/lib/Dicta/dicta-mcp"
else
  echo "Skipping executable smoke for foreign target $target_triple (host: $host_triple)"
fi

mv -f -- "$archive_tmp" "$archive_path"
echo "Linux bundle: $archive_path"
