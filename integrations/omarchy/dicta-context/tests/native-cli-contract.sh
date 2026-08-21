#!/usr/bin/env bash
set -euo pipefail

directory="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
service="$directory/Service.qml"
manifest="$directory/manifest.json"
panel="$directory/Panel.qml"

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
[dictaCommand, "--no-start", "--json", "status"]
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

for input_contract in 'interactive: false' 'id: barActionArea' 'service.toggleRecording()'; do
  grep -Fq -- "$input_contract" "$panel" || {
    echo "Native recording input contract missing: $input_contract" >&2
    exit 1
  }
done

for state_contract in 'visible: service.recordingActive' 'property bool recordingActive: false'; do
  grep -Fq -- "$state_contract" "$panel" "$service" || {
    echo "Native recording-state contract missing: $state_contract" >&2
    exit 1
  }
done

echo "dicta-context native CLI contract: ok"
