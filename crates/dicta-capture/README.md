# dicta-capture

Synchronous capture primitives for Dicta on Omarchy/Hyprland Wayland sessions.

The crate discovers outputs and PipeWire/PulseAudio sources without opening a
capture stream, builds shell-free command plans, and owns the lifecycle of one
recorder process. Backend selection is deterministic: `gpu-screen-recorder`
6.x is primary and `wf-recorder` is the compatibility fallback when the primary
executable is absent. The caller must call `Recorder::poll` regularly; polling
enforces the 20-minute recording limit.

The GPU plan follows Omarchy's built-in screen recorder: codec auto-selection,
60 FPS CFR by default, GPU encoding with explicit CPU fallback, and AAC audio.
Desktop-only, microphone-only, and combined capture use `default_output`,
`default_input`, and `default_output|default_input`. Full monitors use their
connector name. Logical regions use the gpu-screen-recorder 6.x canonical
`-w region -region WxH+X+Y` form. Portal selection is deliberately opt-in and
never silently falls back to `wf-recorder`.

Capture writes to a unique same-directory staging file. A clean stop syncs the
file and promotes it without replacing an existing destination; aborts, failed
stops, and drops reap the child and remove the staging artifact.

This crate deliberately does not support X11, macOS, Spectacle, or an async
runtime. The artifact always retains the selected output/region geometry,
scale, and encoded pixel size for annotation mapping. A portal may let the user
choose a window whose actual bounds differ from the selected output metadata;
callers must not assume exact overlay mapping for that interactive case.

`wf-recorder` accepts one audio source: simultaneous microphone and system
audio therefore keeps using a pre-provisioned combined PipeWire source tagged
with `dicta.capture.role=mixed`. Provisioning that virtual source lives outside
this backend. The GPU backend does not require that virtual source because it
can merge the two default endpoints itself.

Unit tests exercise discovery, command plans, backend selection, lifecycle,
atomic promotion, and cleanup. A real interactive smoke test (monitor, region,
portal, desktop audio, microphone, and merged audio) still requires an active
Hyprland session and is intentionally not run by CI or non-interactive agents.
