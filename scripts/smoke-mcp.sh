#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
executable="${1:-$repo_root/mcp/target/debug/dicta-mcp}"
responses="$(mktemp)"
errors="$(mktemp)"
cleanup() {
  rm -f -- "$responses" "$errors"
}
trap cleanup EXIT INT TERM

{
  printf '{\n'
  printf '%s\n' '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"stdio-smoke","version":"1"}}}'
  printf '%s\n' '{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}'
} | timeout 10 "$executable" >"$responses" 2>"$errors" || {
  status=$?
  echo "$executable failed its stdio smoke (status $status): $(tr '\n' ' ' <"$errors")" >&2
  exit "$status"
}

if [[ "$(wc -l <"$responses")" -ne 3 ]]; then
  echo "Expected 3 MCP responses" >&2
  exit 1
fi
jq -e -s '.[0].error.code == -32700 and .[1].result.serverInfo.name == "dicta"' "$responses" >/dev/null
jq -e -s '
  ([.[2].result.tools[].name] | sort) ==
  (["get_current_project", "get_project_guidance", "get_recording", "get_recording_context", "get_recording_frames", "list_projects", "list_recordings"] | sort)
' "$responses" >/dev/null
echo "MCP stdio smoke: 3 responses, 7 tools"
