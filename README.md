# Dicta

Dicta is a native Linux screen recorder for turning a narrated desktop session
into durable context for coding agents. It records the screen, desktop audio,
microphone, live annotations, a short intent note, local Whisper transcription,
and timestamped review notes into one project-scoped packet.

The current product path is Rust first: a small Qt Quick host owns the native
window and GPU annotation surface, while Rust owns the state machine, capture,
storage, transcription, Unix control socket, CLI, and MCP server. It does not
use a WebView or general async executor.

## Workflow

1. Select **General**, create a standalone project, or link a Git repository.
2. Describe what the recording should explain and press **Record**.
3. Draw with pen, arrow, rectangle, or spotlight while recording. Escape or
   right-click returns the overlay to pointer pass-through mode.
4. Stop from the app, Omarchy bar, global shortcut, or `dicta record stop`.
5. Play the video, inspect its poster and transcript, add notes at the playback
   cursor, or copy the complete context packet for an agent.

Git-linked recordings can be repository-wide or locked to the active branch.
Dicta adds `.dicta/` to the repository's local `.git/info/exclude`; recordings
never need to enter Git history. Merged-branch cleanup proves Git ancestry and
deletes only video files from non-active branches whose recorded revision is
contained in the default branch. Metadata, transcript, and notes remain.

```text
<repo>/.dicta/recordings/<date>/
<repo>/.dicta/branches/<encoded-branch>/recordings/<date>/
<dicta-root>/General/recordings/<date>/
```

Each packet uses the shared versioned Rust model and normally contains an MP4,
JSON metadata, JPEG poster, optional annotation sidecar, and transcript.

## Native Linux requirements

Dicta targets Omarchy/Hyprland first. Building requires Rust, CMake, a C++20
compiler, and Qt 6.5+ development packages for Core, Gui, Qml, Quick, and
optionally Multimedia.

The installed runtime uses:

- `gpu-screen-recorder` for the primary GPU capture path;
- `wf-recorder` as the wlroots fallback;
- PipeWire/PulseAudio-compatible sources for desktop and microphone audio;
- FFmpeg for audio normalization, poster frames, and MCP screenshots;
- Voxtype plus the packaged compact Whisper model for local transcription;
- `wl-copy` only when an explicit CLI copy action is requested.

Missing transcription or playback tooling is reported without disabling the
recording library.

## Build and test

```sh
cmake -S apps/dicta-native -B apps/dicta-native/build \
  -DCMAKE_BUILD_TYPE=Release -DDICTA_BUILD_TESTS=ON
cmake --build apps/dicta-native/build --parallel
cargo build --locked -p dicta-cli
```

Run the native host directly:

```sh
apps/dicta-native/build/dicta-native
```

Or let the CLI start and raise the single host process:

```sh
DICTA_NATIVE_BIN="$PWD/apps/dicta-native/build/dicta-native" \
  target/debug/dicta ui
```

The release gate is documented in
[`docs/native-feature-parity.md`](docs/native-feature-parity.md). The core local
checks are:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
ctest --test-dir apps/dicta-native/build --output-on-failure
cmake --build apps/dicta-native/build --target dicta-native_qmllint
bash scripts/check-versions.sh
```

Unix-socket and live Hyprland tests need an environment that permits private
`AF_UNIX` sockets and access to the active compositor.

## CLI

The `dicta` CLI speaks the typed local protocol over a private per-user Unix
socket. Read operations can fall back to the persisted catalog when the app is
not running.

```sh
dicta ui
dicta status
dicta record start --note "Explain the overlay focus bug"
dicta annotate enable
dicta annotate tool arrow
dicta record stop
dicta project list
dicta recording list --project dicta
dicta recording open <recording-id>
dicta context <recording-id> --project dicta --copy
dicta model status
dicta model install quality
dicta settings get
dicta doctor
```

Use `dicta --help` for the complete grammar and `--json` for stable automation
output.

## Omarchy integration

The native UI follows Omarchy rather than maintaining a competing theme. It
reads the active palette, shell scale, and monospace font and reloads them while
the app is running.

The native archive includes two optional integrations:

```sh
dicta-install-omarchy-plugin
dicta-install-omarchy-shortcut
```

The first installs the project/context/recording bar plugin. The second adds one
isolated `~/.config/hypr/dicta-bindings.lua` module, validates `hyprctl reload`,
and tracks the shortcut preset selected in Dicta settings. It is reversible:

```sh
dicta-install-omarchy-shortcut --remove
```

The installer backs up the touched binding file and never modifies Omarchy's
system-owned source tree.

## MCP

`dicta-mcp` is a standalone read-only stdio server. It does not require the app
or control socket to be running. The native archive installs it at
`/usr/lib/Dicta/dicta-mcp` and exposes:

- `list_projects`
- `get_current_project`
- `get_project_guidance`
- `list_recordings`
- `get_recording`
- `get_recording_context`
- `get_recording_frames`

Frame requests return up to eight real inline JPEGs extracted from a confined,
non-symlinked recording. No deleted temporary path is exposed.

## Package

Build the system-Qt native archive on the target Linux architecture:

```sh
bash scripts/package-native-linux.sh
```

The reproducible archive contains the Qt/Rust host, CLI, MCP server, compact
model, desktop entry, icons, Omarchy integration, and documentation. It rejects
web-runtime dependencies and bundled shared libraries during packaging.

## Migration status

Dicta is Linux-native. The release, CI, desktop launcher, CLI, MCP helper, and
Omarchy integrations all build from the Rust/Qt implementation.
