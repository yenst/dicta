# Dicta

Dicta is a macOS menu-bar app for recording screen + voice explanations into project folders that can be handed to coding agents as prompt context. Narration is transcribed automatically: Dicta tries macOS Speech Recognition first and falls back to local Whisper when Siri or Dictation is unavailable. Both use the default language from **Settings → Transcription** (Auto-detect unless you pick one). A compact model ships inside the app, while the same settings page can download and verify the much better multilingual `large-v3-turbo-q5_0` model into `~/Library/Application Support/Dicta/models/`. Pending (not failed) transcriptions are retried when Dicta opens.

## MVP workflow

1. Link a Git working-copy folder or select an existing project.
2. Add a short note describing what the model should learn.
3. Use **Settings → Shortcuts** to choose a global shortcut, then press it to start or stop recording without bringing Dicta forward. The tray title shows ● while recording. The in-app Record button still opens the note sheet.
4. Press Record in the sheet, or press the shortcut again to stop. Recordings cap at 20 minutes.
5. Open a packet to play the video and read the transcript. Use **Context** to copy a Markdown index of notes and artifact paths.

Dicta reads the working copy's current branch whenever the app is focused, a project is selected, or recording starts. Linked-project recordings live inside the authorized Git workspace so Codex and other agents can read them without requesting access to an unrelated macOS folder:

```text
<repo>/.dicta/branches/<branch>/recordings/<date>/
```

Dicta adds `.dicta/` to the repository's local `.git/info/exclude`, so videos never appear in Git status or get committed. Each recording is an `.mp4` plus a `.json` sidecar. Branch names keep their exact value in metadata; `/` is encoded as `__` only in folder names (`feature/oauth` becomes `feature__oauth`). Changing Git branches switches the visible packet shelf and the recording destination.

Merged-video cleanup is available in **Settings → Storage**. Press **Clean** to use Git ancestry to confirm that a recorded branch tip is contained in the repository's default branch, then remove only that branch's `.mp4` files. Transcripts, notes, and metadata remain available to MCP. The active and default branches are never cleaned. Cleanup never runs just because the window regained focus.

When a project is linked after upgrading, Dicta safely copies any existing packets from `~/Documents/Dicta/<project>` into `<repo>/.dicta` and rewrites their evidence paths. The original library is left intact.

## Run locally

Requirements: macOS 15+, Xcode Command Line Tools, Rust, and Node.js.

```bash
npm install
npm run tauri dev
```

## MCP access

Dicta bundles a standalone, read-only MCP server and installs it to:

```text
~/Library/Application Support/Dicta/bin/dicta-mcp
```

Select **Connect Codex** in Dicta to register it with Codex. Then link the Git project once so its repository-local `.dicta` index is created. If a loaded Codex task reports `Transport closed`, use **Restart Codex MCP** in Dicta; it reinstalls the server atomically and forces Codex to refresh its MCP transports. You can also configure any stdio-compatible agent manually with that executable path.

The server exposes:

- `get_project_guidance` — resolve a repository and current branch, then return the most relevant recorded guidance.
- `list_recordings` — browse a branch's recordings newest-first.
- `get_recording` — read one recording's note, metadata, transcript when available, and evidence paths.
- `get_recording_frames` — extract up to eight timestamped JPEG screenshots from a recording and return them inline. Pass exact timestamps or let Dicta sample useful moments across the video.

Example prompt:

```text
Check Dicta for this project and current branch for prior guidance, then implement this ticket.
```

Dicta tools never modify recordings or repositories.

Frame extraction uses the original local MP4 and macOS AVFoundation. Screenshots are written only to Dicta's temporary cache and returned as MCP image content. When word-level timing is unavailable, any nearby transcript excerpt is explicitly marked as approximate position-based context; the screenshot timestamp itself is exact.

The first recording asks for Microphone and Screen Recording permission. macOS may require the app to be restarted after Screen Recording permission is granted.

## Current scope

- Main display only
- Four configurable global recording shortcuts
- MP4 + metadata prompt packets
- Manual Git-verified video cleanup for merged branches
- Downloadable high-quality local transcription model with progress and SHA-1 integrity verification
- Default transcription language in Settings, used for Speech Recognition and the Whisper fallback
- In-app video playback, transcript view, and poster frames
- MCP `get_recording_frames` for up to eight timestamped screenshots

Double-tapping bare `Fn` needs a lower-level macOS event tap plus Accessibility/Input Monitoring permission. Dicta currently offers reliable Command, Option, and Control combinations instead.
