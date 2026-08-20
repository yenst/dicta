# Dicta CLI

`dicta` is the thin command-line client for the native Dicta process. It parses
the invocation before touching the control socket and uses the typed v1 grammar
from `dicta-control`.

## Process model

Online commands probe the private Unix socket first. If it is absent, the CLI
starts `dicta-native --background --socket PATH` directly (never through a
shell), waits up to two seconds for that exact socket, and sends the original command. Set
`DICTA_NATIVE_BIN`, `DICTA_SOCKET`, or use their matching command-line options
to override discovery. `--no-start` makes socket absence an immediate exit 69.

`context --copy` asks the server only for context and pipes the returned text to
`wl-copy` in the client. This keeps clipboard policy out of protocol v1.

Persistent preferences use the typed native control surface as well:

```sh
dicta settings get
dicta settings language nl
dicta settings shortcut control_space
dicta settings branch-locking off
dicta settings cleanup on
dicta settings cleanup-now
dicta settings cleanup-now --project PROJECT_ID
dicta settings general-path /data/Dicta-General
```

The response is the complete legacy-compatible settings document, in human or
`--json` form, so updates are observable without reading files directly.

## Current boundary

When the socket is absent, `recording list`, `recording show`, and `context`
fall back to read-only storage access instead of starting the GUI. The loader
uses typed core metadata, checks symlink/confinement boundaries, reads transcript
sidecars, and covers registered projects, repository-local `.dicta` storage,
branch packets, General, and the legacy unprojected directory. `DICTA_HOME`
overrides the default `~/Documents/Dicta`/legacy `PromptReel` discovery.

MCP mode, offline mutation, shell completions, and daemon ownership/lifecycle
commands are not implemented yet. Those can be added without changing the
transport, offline-store, or host seams used by the tests.
