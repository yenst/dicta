# Dicta native host

`dicta-native` is the single-process Linux host for Dicta. Qt Quick owns the
windowing and scene-graph overlay, while a synchronous Rust service thread owns
the recording runtime, local control socket, capture ports, annotation state,
and atomic recording metadata. There is no WebView process or async
runtime in this path.

The production constructor discovers the requested Hyprland output through
`dicta-linux` and uses the real Linux capture port, with gpu-screen-recorder as
its primary recorder. After capture, the runtime nonblockingly queues local
transcription through FFmpeg and the installed Voxtype Whisper backend when a
compatible Dicta model is available. Missing transcription tools disable that
step explicitly without breaking recording.

## Build

Requirements are CMake 3.21+, Cargo, a Rust toolchain, a C++20 compiler, and Qt
6.5+ with Core, Gui, Qml, and Quick development packages.

The installed runtime expects Omarchy/Hyprland plus `gpu-screen-recorder`,
FFmpeg, and Voxtype. `wf-recorder` remains the capture fallback. Qt Multimedia
enables in-app playback when installed; the recording library and transcript
remain usable without it. `wl-clipboard` is used only for explicit CLI context
copy actions.

```sh
cmake -S . -B build -DCMAKE_BUILD_TYPE=Release
cmake --build build
```

CMake tracks the bridge's Rust sources and each local Rust dependency, so a
change in `host.rs` or a concrete port rebuilds the static archive. The native
executable is `build/dicta-native`.

## Service mode

The CLI auto-starts the host with:

```sh
./build/dicta-native --background
```

The native process derives explicit defaults for all runtime inputs:

- socket: `$DICTA_SOCKET`, otherwise `$XDG_RUNTIME_DIR/dicta/control-v1.sock`;
- storage: `$DICTA_STORAGE_ROOT`, then `$DICTA_HOME`, otherwise the preferred
  `~/Documents/Dicta` root;
- output: `$DICTA_OUTPUT`, otherwise Qt's primary output name.

They can also be set with `--socket`, `--storage-root`, and `--output`.
Shutdown requests interrupt idle and partially connected control clients,
remove the socket, join the Rust service thread, and only then destroy the Qt
overlay callback context.

Recording start creates the overlay session and recording-relative clock.
Annotation enable/disable, tool, undo, and clear commands mutate the same Rust
session that receives normalized strokes from the Qt scene-graph item. Stop
finalizes that session and atomically persists the versioned annotation
sidecar beside the recording metadata. A shell-free FFmpeg step also extracts
one no-clobber JPEG poster beside successful captures; the Qt viewer uses it
while the video decoder is preparing. Typed timeline notes add or remove marks
at the current playback cursor and atomically update the same catalog model.

The settings screen reads and atomically updates the same `settings.json`
contract as the legacy app. Shortcut preset, transcription language, branch
locking, merged-video cleanup policy, and the General path are available through
the Qt screen and typed `dicta settings` commands. Language changes apply to
future local transcription jobs without restarting the host. Omarchy remains
the appearance owner: Dicta reads its current palette, shell scale, and
monospace font live instead of persisting a competing light/dark preference.

Persisting a shortcut preset does not silently edit the user's Hyprland files;
the installed Omarchy binding is a separate integration step. The cleanup
policy is enforced by the typed `dicta settings cleanup-now` command and the
settings-screen action. Cleanup proves that a non-active branch revision is
merged before deleting only regular video artifacts; branch metadata,
transcripts, notes, and unmerged/default/active-branch recordings are kept.

## Overlay placement boundary

The transparent overlay is deliberately functional and unstyled. Drawing is a
GPU-backed `QQuickItem`; it does not use `QQuickPaintedItem`. The isolated
placement port selects the requested `QScreen`, maps a frameless fullscreen
Hyprland bypass/tool surface, keeps it above ordinary windows, and switches
between click-through and focused annotation modes synchronously.

## Verification

```sh
cargo fmt --all -- --check
cargo clippy -p dicta-native-bridge --all-targets -- -D warnings
cargo test -p dicta-native-bridge --all-targets -- --test-threads=1

cmake -S . -B build -DCMAKE_BUILD_TYPE=Release -DDICTA_BUILD_TESTS=ON
cmake --build build
ctest --test-dir build --output-on-failure
cmake --build build --target dicta-native_qmllint
```

The Rust E2E drives the control socket, injects a normalized stroke, verifies
its persisted sidecar, joins the host, and verifies socket cleanup. The native
process E2E uses the built CLI to auto-start the built Qt executable with a fake
capture platform, then checks status/start/stop, artifacts, and deterministic
process/socket shutdown. Its fake platform is enabled only by
`--e2e-fake-capture` or `DICTA_NATIVE_E2E=1`; production never selects it.

The control-socket E2Es need permission to create Unix sockets. Sandboxes that
deny `bind(2)` must run those tests outside that restriction.

## Native Linux archive

From the repository root, build the system-Qt native archive with:

```sh
bash scripts/package-native-linux.sh
```

The archive contains `dicta-native`, the Rust `dicta` CLI, the read-only MCP
helper, compact Whisper model, desktop entry, icons, and optional Omarchy bar
plugin. It deliberately does not include a web runtime or bundled Qt libraries.
Build it on the Linux architecture where it will run; cross-
compiling the Qt host is rejected until a supported Qt toolchain file exists.

## Omarchy recording shortcut

Dicta delegates the global shortcut to Hyprland instead of linking another
desktop-wide input library. Install the isolated, reversible binding module
once after installing the native archive:

```sh
dicta-install-omarchy-shortcut
```

The helper reads the persisted shortcut preset, backs up and adds one module
include to `~/.config/hypr/bindings.lua`, writes only
`~/.config/hypr/dicta-bindings.lua`, then runs `hyprctl reload` and
`hyprctl configerrors`. The module unbinds the selected key before binding the
same typed `dicta record toggle` command used by the dashboard and Omarchy bar.
Later settings changes atomically update only that managed module. Remove it
with `dicta-install-omarchy-shortcut --remove`; the underlying binding becomes
active again after reload.

Use `--shortcut control_space` (or another supported preset) to override the
stored value during installation. The native CLI contract uses
`dicta record toggle`.
