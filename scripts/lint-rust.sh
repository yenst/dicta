#!/usr/bin/env bash
set -euo pipefail

project_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$project_root"

cargo fmt --manifest-path crates/dicta-core/Cargo.toml -- --check
cargo fmt --manifest-path mcp/Cargo.toml -- --check
cargo fmt --manifest-path src-tauri/Cargo.toml -- --check
cargo clippy --locked --manifest-path src-tauri/Cargo.toml -p dicta-core --all-targets -- -D warnings
cargo clippy --locked --manifest-path mcp/Cargo.toml --all-targets -- -D warnings
cargo clippy --locked --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
