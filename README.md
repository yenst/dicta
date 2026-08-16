# Dicta

Dicta is a macOS and Linux desktop app for recording screen + voice explanations into project folders that can be handed to coding agents as prompt context. Narration is transcribed automatically: macOS tries Speech Recognition first, while Linux uses the bundled local Whisper engine directly. Both use the default language from **Settings → Transcription** (Auto-detect unless you pick one). A compact model ships inside the app, while the same settings page can download and verify the much better multilingual `large-v3-turbo-q5_0` model. Pending (not failed) transcriptions are retried when Dicta opens.

## MVP workflow

1. Link a Git working-copy folder or select an existing project.
2. Add a short note describing what the model should learn.
3. Use **Settings → Shortcuts** to choose a global shortcut, then press it to start or stop recording without bringing Dicta forward. The tray title shows ● while recording. The in-app Record button still opens the note sheet.
4. Press Record in the sheet, or press the shortcut again to stop. Recordings cap at 20 minutes.
5. Open a packet to play the video and read the transcript. Use **Context** to copy a Markdown index of notes and artifact paths.

Dicta reads the working copy's current branch whenever the app is focused, a project is selected, or recording starts. Linked-project recordings live inside the authorized Git workspace so Codex and other agents can read them without requesting access to an unrelated app-data folder:

```text
<repo>/.dicta/branches/<branch>/recordings/<date>/
```

Dicta adds `.dicta/` to the repository's local `.git/info/exclude`, so videos never appear in Git status or get committed. Each recording is an `.mp4` plus a `.json` sidecar. Branch names keep their exact value in metadata; `/` is encoded as `__` only in folder names (`feature/oauth` becomes `feature__oauth`). Changing Git branches switches the visible packet shelf and the recording destination.

Merged-video cleanup is available in **Settings → Storage**. Press **Clean** to use Git ancestry to confirm that a recorded branch tip is contained in the repository's default branch, then remove only that branch's `.mp4` files. Transcripts, notes, and metadata remain available to MCP. The active and default branches are never cleaned. Cleanup never runs just because the window regained focus.

When a project is linked after upgrading, Dicta safely copies any existing packets from `~/Documents/Dicta/<project>` into `<repo>/.dicta` and rewrites their evidence paths. The original library is left intact.

## Run locally

Requirements on every platform: Rust, Node.js, and the native Tauri 2 build dependencies.

- macOS: macOS 15+ and Xcode Command Line Tools.
- Linux: FFmpeg, PulseAudio/PipeWire-Pulse, and a supported capture backend. KDE Plasma Wayland uses Spectacle, wlroots Wayland uses `wf-recorder`, and X11 uses FFmpeg's `x11grab`. Clipboard actions additionally need `wl-clipboard`, `xclip`, or `xsel`. In-app playback needs GStreamer's libav plugin (`gst-libav`).

On Arch/CachyOS, install the common runtime and build packages with:

```bash
sudo pacman -S --needed base-devel cmake clang curl ffmpeg gst-libav gst-plugins-good nodejs npm rust shaderc vulkan-headers vulkan-icd-loader webkit2gtk-4.1 libayatana-appindicator librsvg wl-clipboard xorg-xrandr
```

For wlroots compositors such as Sway or Hyprland, also install `wf-recorder`. On KDE Wayland, Plasma asks you to click a window on the screen you want to record after starting; Dicta then records that entire screen. On X11, `xrandr` is used to detect the desktop size. Linux capture can be forced with `DICTA_SCREEN_RECORDER=spectacle`, `wf-recorder`, or `ffmpeg-x11` when auto-detection is not appropriate.

Linux transcription uses Whisper's Vulkan backend when a compatible GPU and Vulkan driver are available, with the CPU backend retained as a fallback. `shaderc` and `vulkan-headers` are build-time requirements; packaged Dicta binaries only need the Vulkan loader and a working vendor driver.

On NVIDIA Wayland sessions, Dicta automatically disables WebKitGTK's DMA-BUF renderer to avoid a startup protocol error. Set `DICTA_ENABLE_WEBKIT_DMABUF=1` before launch only if you deliberately want to test the native renderer again.

```bash
npm install
npm run tauri dev
```

Build the native Linux archive with:

```bash
npm run bundle:linux
```

The archive uses the host GTK/WebKit stack and includes Dicta, its MCP helper, the compact Whisper model, desktop metadata, and application icons. This is the recommended package on rolling-release distributions such as Arch and CachyOS, where linuxdeploy's bundled WebKit or binutils can be incompatible with current system libraries.

## MCP access

Dicta bundles a standalone, read-only MCP server and installs it to:

```text
macOS: ~/Library/Application Support/Dicta/bin/dicta-mcp
Linux: ~/.local/share/Dicta/bin/dicta-mcp
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

Frame extraction uses the original local MP4 with macOS AVFoundation or Linux FFmpeg. Screenshots are written only to Dicta's temporary cache and returned as MCP image content. When word-level timing is unavailable, any nearby transcript excerpt is explicitly marked as approximate position-based context; the screenshot timestamp itself is exact.

The first recording may ask for Microphone and Screen Recording permission. macOS may require the app to be restarted after Screen Recording permission is granted; Wayland displays a desktop capture permission prompt through its native recorder.

## Current scope

- Main display only
- Four configurable global recording shortcuts
- MP4 + metadata prompt packets
- Manual Git-verified video cleanup for merged branches
- Downloadable high-quality local transcription model with progress and SHA-1 integrity verification
- Default transcription language in Settings, used for Speech Recognition and local Whisper
- In-app video playback, transcript view, and poster frames
- MCP `get_recording_frames` for up to eight timestamped screenshots

Double-tapping bare `Fn` needs a lower-level macOS event tap plus Accessibility/Input Monitoring permission. Dicta offers Command/Option/Control combinations on macOS. Linux defaults to `Alt+Shift+R`, with additional Super/Alt/Control combinations in Settings.
