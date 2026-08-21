# Native feature-parity ledger

The Linux-native Rust/Qt product path owns every shipped Dicta capability.

| Capability | State | Linear | Verification |
| --- | --- | --- | --- |
| GPU screen capture, mixed desktop + mic audio, bounded lifecycle | Complete | PLC-167, PLC-175 | Capture contract, real recorder cleanup, and runtime tests |
| Live annotations and fullscreen click-through overlay | Complete | PLC-168 | Qt scene-graph, persistence, input-mode, and Hyprland placement tests |
| Local Whisper, model/language controls, retry and retranscription | Complete | PLC-169 | Nonblocking worker, fixture, persistence, retry, and cancellation tests |
| Dashboard, settings, projects, Git branch/worktree scope | Complete | PLC-170, PLC-180 | QML tests, catalog E2E, real linked-worktree tests, and atomic settings tests |
| Viewer, poster, transcript, timed text and voice notes | Complete | PLC-171, PLC-181 | Qt viewer tests, real JPEG extraction, atomic note persistence, and bounded voice capture tests |
| Typed CLI, background host, read-only MCP, Codex controls | Complete | PLC-172, PLC-182 | Socket process E2E, MCP stdio smoke, exact Codex argv and rollback tests |
| Omarchy bar project/context/record controls | Complete | PLC-173 | Native CLI contract and confined installer tests |
| Native archive, desktop entry, model, icons and integrations | Complete | PLC-174 | Reproducible package build, archive inspection, and packaged binary smokes |

## Release gate

```sh
bash scripts/check-versions.sh
bash scripts/lint-rust.sh
bash scripts/test-rust.sh
cmake --build apps/dicta-native/build --parallel
ctest --test-dir apps/dicta-native/build --output-on-failure
cmake --build apps/dicta-native/build --target dicta-native_qmllint
bash scripts/package-native-linux.sh
```

Unix-socket tests require permission to bind private `AF_UNIX` sockets. Live
capture additionally requires the active Hyprland session, recorder tools,
audio sources, and the selected local transcription model.
