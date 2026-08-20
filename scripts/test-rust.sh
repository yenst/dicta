#!/usr/bin/env bash
set -euo pipefail

project_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$project_root"

# Exercise the library through a binary crate's committed dependency lock.
cargo test --locked --manifest-path src-tauri/Cargo.toml -p dicta-core
cargo test --locked --manifest-path mcp/Cargo.toml
cargo test --locked --manifest-path src-tauri/Cargo.toml
