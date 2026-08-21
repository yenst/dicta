#!/usr/bin/env bash
set -euo pipefail

project_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$project_root"

cargo fmt --all -- --check
cargo fmt --manifest-path mcp/Cargo.toml -- --check
cargo clippy --locked --workspace --all-targets -- -D warnings
cargo clippy --locked --manifest-path mcp/Cargo.toml --all-targets -- -D warnings
