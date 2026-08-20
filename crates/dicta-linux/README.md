# dicta-linux

Concrete synchronous Linux composition for Dicta's native runtime.

`LinuxConfig` requires an explicit absolute storage root and an exact discovered
Hyprland output name. `build_runtime` wires `gpu-screen-recorder`-first capture,
atomic core-model storage, the system clock, filesystem-reserved recording IDs,
and a bounded local transcription worker. The production audio default combines
`default_output|default_input` for desktop sound and narration.
`build_runtime_with_observer` adds a capture-start hook intended for the native
overlay's visibility and relative timestamp clock.

Recordings use the existing offline-scanner-compatible shape:

```text
<root>/<project-or-General>/recordings/YYYY-MM-DD/<recording-id>.mp4
<root>/<project-or-General>/recordings/YYYY-MM-DD/<recording-id>.json
<root>/<project-or-General>/recordings/YYYY-MM-DD/<recording-id>.poster.jpg
```

## Local transcription settings

Local transcription is enabled by default when an existing Dicta Whisper model,
`ffmpeg`, and `voxtype` are all available. Missing tooling or models disables it
gracefully without disabling recording. Language selection is resolved from
`DICTA_TRANSCRIPTION_LANGUAGE`, then
`<storage-root>/settings.json`'s `transcription_language`, then `auto`.

- `DICTA_TRANSCRIPTION=0|false|off|disabled` disables transcription.
- `DICTA_TRANSCRIPTION_MODEL=auto|compact|large-v3-turbo|<absolute-path>` selects a model.
- `DICTA_WHISPER_MODEL` preserves the legacy explicit model override.
- `DICTA_BUNDLED_WHISPER_MODEL` locates a packaged compact model.
- `DICTA_FFMPEG_BIN` and `DICTA_VOXTYPE_BIN` override executable paths.
- `DICTA_CURL_BIN` and `DICTA_SHA1SUM_BIN` override the model install tools.

The worker extracts 16 kHz mono WAV audio, stages the selected model through an
isolated temporary `XDG_DATA_HOME`, and invokes Voxtype with structured arguments.
Submission returns immediately; the native service polls and persists completion
without blocking CLI or UI requests.

`dicta model status` reports the active model, the managed quality-model path,
integrity state, and current install progress. `dicta model install quality`
starts a single background download. It invokes `curl` and `sha1sum` directly
with structured arguments, writes a same-directory partial, validates the
catalog SHA-1 and minimum size, then atomically promotes the verified file.
Duplicate installs and installs while recording/transcribing return a typed
conflict without blocking the control service.

Production startup scans recording metadata on a bounded background thread.
Successful recordings left in `pending` or `failed` transcription state are
retried one at a time once the runtime is idle and transcription tooling is
available. The pending state is saved before inference and failures remain
restart-retryable.

## Current limits

Voxtype's file transcription output has no timed segments or reliable
detected-language field. Model installation currently exposes byte progress by
polling the partial file rather than curl's transfer-rate/ETA stream. The native
settings screen exposes language, storage, shortcut, branch and cleanup policy,
plus the same nonblocking quality-model installation/status path as the CLI.
Manual retranscription is available from both the recording viewer and CLI.

The crate contains no UI toolkit, global state, shell command construction, or
async runtime. `dicta-capture` owns shell-free process plans and recorder
lifecycle. Integration tests use fake platform/process implementations. Real
Wayland/PipeWire capture and the `wf-recorder` fallback's provisioned mixed source
remain interactive environment gates.
