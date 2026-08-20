# Native feature-parity ledger

The native Linux rewrite replaces the legacy Tauri application only when every
row below is implemented through the Rust/Qt product path and has its stated
verification evidence. A protocol enum, QML placeholder, or fake-only adapter
does not count as parity.

| Capability | Native state | Linear | Required evidence |
| --- | --- | --- | --- |
| GPU screen capture, mixed desktop + mic audio, 20-minute bound | Complete | PLC-167, PLC-175 | Real Hyprland H.264/AAC recording and recorder cleanup |
| Live pen/arrow/rectangle/spotlight annotations | Partial | PLC-168 | Real drawing, sidecar persistence, undo/clear, reliable pointer escape; layer-shell still required |
| Recording dashboard and note-first start/stop | Complete | PLC-170 | Dashboard QML test and built CLI-to-native process E2E |
| Project add/select/remove and Git branch scope | In progress | PLC-172 | Production storage catalog tests and repository-local capture-path/metadata E2E are green; interactive repository recording remains |
| Recording list/show/delete/open and context copy | In progress | PLC-171, PLC-172 | Production catalog E2E plus destructive-boundary tests |
| Local Whisper, language selection, retry and retranscription | In progress | PLC-169 | Real speech fixture, nonblocking service, transcript persistence and restart retry |
| Video viewer, poster, transcript and timed notes | In progress | PLC-171 | Qt Multimedia playback and transcript/chapter views are live; typed notes now add/remove at the playback cursor and persist atomically through the catalog. Shell-free FFmpeg poster extraction is no-clobber and real-JPEG tested. Final live viewer interaction remains |
| Settings: shortcut, language/model, storage, cleanup, integrations | In progress | PLC-170 | Legacy-compatible atomic settings, typed CLI/protocol, live language update, nonblocking quality-model install/status, safe merged-branch cleanup and Omarchy-styled Qt controls are tested; live shortcut activation remains |
| Global shortcut and recording status integration | In progress | PLC-170, PLC-173 | Reversible managed Omarchy binding and live preset sync are implemented with confinement tests; background-host live keypress test remains |
| Omarchy bar project/context/record controls | In progress | PLC-173 | Native CLI-only panel now uses typed titles/timestamps and exact recording navigation; live installed-plugin interaction remains |
| Read-only MCP project/recording/frame tools | Complete | PLC-172 | All seven typed read-only tools pass the stdio E2E; the repository fixture extracts a real inline JPEG frame with FFmpeg and exposes no deleted temp path |
| Native archive, desktop entry, model and plugin | Partial | PLC-174 | Clean install/uninstall test; archive build and self-smoke already pass |
| Legacy Tauri/WebKit/macOS removal | Blocked on rows above | PLC-174 | Release tag, migrated CI, no obsolete packaged/runtime dependency |

## Current release gate

Run all of the following before changing a row to complete:

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
cmake --build apps/dicta-native/build
ctest --test-dir apps/dicta-native/build --output-on-failure
cmake --build apps/dicta-native/build --target dicta-native_qmllint
node scripts/check-versions.mjs
npm run bundle:native-linux
```

Unix-socket E2Es must run in an environment that permits private `AF_UNIX`
bind/connect operations. Interactive Hyprland, capture, playback, and speech
checks cannot be replaced by offscreen or fake-platform tests.
