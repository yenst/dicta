#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

cargo_version() {
  sed -n '/^\[package\]/,/^\[/s/^version[[:space:]]*=[[:space:]]*"\([^"]*\)".*/\1/p' "$1" | head -n 1
}

cmake_version() {
  sed -n 's/.*project([^)]*VERSION[[:space:]]\+\([^[:space:])]*\).*/\1/ip' "$1" | head -n 1
}

manifest_version() {
  sed -n 's/.*"version"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p' "$1" | head -n 1
}

expected="$(cargo_version apps/dicta-cli/Cargo.toml)"
if [[ -z "$expected" ]]; then
  echo "Could not read the canonical version from apps/dicta-cli/Cargo.toml" >&2
  exit 1
fi

declare -a declarations=(
  "apps/dicta-cli/Cargo.toml:$expected"
  "apps/dicta-native/rust/Cargo.toml:$(cargo_version apps/dicta-native/rust/Cargo.toml)"
  "crates/dicta-core/Cargo.toml:$(cargo_version crates/dicta-core/Cargo.toml)"
  "mcp/Cargo.toml:$(cargo_version mcp/Cargo.toml)"
  "apps/dicta-native/CMakeLists.txt:$(cmake_version apps/dicta-native/CMakeLists.txt)"
  "integrations/omarchy/dicta-context/manifest.json:$(manifest_version integrations/omarchy/dicta-context/manifest.json)"
)

failed=0
for declaration in "${declarations[@]}"; do
  path="${declaration%%:*}"
  version="${declaration#*:}"
  if [[ "$version" != "$expected" ]]; then
    echo "$path has version ${version:-<missing>}; expected $expected" >&2
    failed=1
  fi
done
((failed == 0)) || exit 1

if [[ "${1:-}" == "--print" ]]; then
  printf '%s' "$expected"
else
  echo "Version consistency: $expected across ${#declarations[@]} declarations"
fi
