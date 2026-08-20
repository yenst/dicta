import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";

const directory = join(dirname(fileURLToPath(import.meta.url)), "..");
const service = readFileSync(join(directory, "Service.qml"), "utf8");
const manifest = JSON.parse(readFileSync(join(directory, "manifest.json"), "utf8"));

for (const legacy of [
  "dicta-mcp",
  "helperCommand",
  "--show-project",
  "--show-recording",
  "--toggle-recording",
  '["bash",',
  "Util.shellQuote",
  '["pgrep", "-x", "dicta"]',
]) {
  assert.equal(service.includes(legacy), false, `legacy integration remains: ${legacy}`);
}

for (const contract of [
  '["pgrep", "-x", "dicta-native"]',
  '[dictaCommand, "ui"]',
  '[dictaCommand, "record", "toggle"]',
  '[dictaCommand, "recording", "open", recording]',
  '[dictaCommand, "--no-start", "--json", "project", "list"]',
  '"--project", selectedProjectId, "--limit", "3"',
  'dictaCommand, "--no-start", "context", recording,',
  '"--project", project, "--copy"',
  'String(recording.note || recording.transcript_preview || recording.id || "Untitled recording")',
  'String(recording.started_at || "")',
]) {
  assert.equal(service.includes(contract), true, `native CLI contract missing: ${contract}`);
}

assert.equal("helperCommand" in manifest.barWidget.defaults, false);
assert.equal(
  manifest.barWidget.schema.some((entry) => entry.key === "helperCommand"),
  false,
);

console.log("dicta-context native CLI contract: ok");
