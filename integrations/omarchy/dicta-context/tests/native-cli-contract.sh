#!/usr/bin/env bash
set -euo pipefail

directory="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
service="$directory/Service.qml"
manifest="$directory/manifest.json"

for legacy in dicta-mcp helperCommand --show-project --show-recording --toggle-recording 'Util.shellQuote'; do
  if grep -Fq -- "$legacy" "$service" "$manifest"; then
    echo "Legacy integration remains: $legacy" >&2
    exit 1
  fi
done

while IFS= read -r contract; do
  grep -Fq -- "$contract" "$service" || {
    echo "Native CLI contract missing: $contract" >&2
    exit 1
  }
done <<'CONTRACTS'
["pgrep", "-x", "dicta-native"]
[dictaCommand, "ui"]
[dictaCommand, "record", "toggle"]
[dictaCommand, "recording", "open", recording]
[dictaCommand, "--no-start", "--json", "project", "list"]
"--project", selectedProjectId, "--limit", "3"
dictaCommand, "--no-start", "context", recording,
"--project", project, "--copy"
String(recording.note || recording.transcript_preview || recording.id || "Untitled recording")
String(recording.started_at || "")
CONTRACTS

echo "dicta-context native CLI contract: ok"
