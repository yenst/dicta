# Dicta native architecture

Dicta is one Linux-native product with a Rust domain core, a thin Qt Quick
presentation layer, and a first-class local control protocol.

## Boundaries

| Component | Owns | Must not own |
| --- | --- | --- |
| `dicta-core` | Persisted IDs, recordings, transcripts, annotations, storage rules | UI or worker processes |
| `dicta-engine` | The single mutable application state machine | Qt, sockets, capture, transcription |
| `dicta-control` | Versioned wire types, CLI grammar, bounded NDJSON, private Unix sockets | Product state or platform behavior |
| `dicta-capture` | Wayland output/audio discovery and one recorder lifecycle | UI or persisted metadata |
| `dicta-transcribe` | One bounded lazy transcription worker | UI or recording state transitions |
| native app | Qt event loop, adapters, worker ownership, presentation | Persisted model duplication |
| CLI | Human/JSON rendering and local socket client behavior | Qt initialization |

Wire types deliberately remain separate from domain types. The native app
validates strings from the local protocol into typed IDs at the engine boundary.
This keeps the control crate reusable and prevents dependency cycles.

## Runtime shape

The Qt main thread owns presentation only. A single controller serializes all
commands and produces immutable snapshots plus work-request events. Capture,
transcription, and local-socket I/O use bounded blocking workers; the project
does not need a general async runtime.

Only the controller assigns event sequence numbers. Socket connections relay
those events and never create their own product timeline.

The control socket lives at `$XDG_RUNTIME_DIR/dicta/control-v1.sock`. Its parent
directory is owned by the current user with mode `0700`; the socket uses mode
`0600`. Startup must probe an existing socket and prove it stale before removing
it.

The packaged host should follow Omarchy's Voxtype lifecycle: a systemd user
service attached to `graphical-session.target`, with restart-on-failure and
`XDG_RUNTIME_DIR=%t`. The CLI may launch a development host directly, but that is
not the final daemon ownership model. Quickshell consumes a streaming status or
event process whose lifetime is tied to the bar, rather than polling or leaving
orphan followers.

## Capture backend policy

Omarchy's built-in recorder uses `gpu-screen-recorder`, including hardware-first
encoding, CPU fallback, and merged desktop plus microphone input. Dicta should
prefer that installed path when available and retain the simpler, already
verified `wf-recorder` implementation as a fallback.

Full-monitor capture is preferred when the selection matches an output. Logical
region geometry stays in compositor coordinates for overlay mapping. Portal
capture remains opt-in for HDR, external-GPU displays, or window capture because
it has different DMA-BUF compatibility tradeoffs.

## Recording and annotation data

Existing 0.8.x recording JSON remains readable in place. New fields are
optional, and unknown fields survive a load/write round trip.

Live drawing is stored next to recording metadata as a versioned
`*.annotations.json` sidecar. Coordinates are normalized to the selected output
and each event carries monotonic recording-relative timestamps. This keeps
annotations independent from the video codec and lets the viewer render or
export them later.

Metadata and annotation files use same-directory atomic replacement. Repository
and branch storage semantics continue to come from `dicta-core`.

## Performance rules

- No WebView in the native process.
- No model or capture initialization during idle startup.
- System Qt libraries are dynamically linked.
- External programs are invoked with structured argument arrays, never shell
  command strings derived from metadata.
- Large transcription models are loaded lazily by one worker and released after
  a configured idle period.
- Platform scope is Omarchy/Hyprland Wayland; X11 compatibility is not a
  constraint on the implementation.

## Migration rule

Native components are verified together across recording, playback, CLI/MCP,
packaging, and persisted-data compatibility.

The executable parity gates and current migration state are tracked in
[`native-feature-parity.md`](native-feature-parity.md). Fake adapters and wire
grammar alone never satisfy a feature-parity row.
