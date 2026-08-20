# dicta-runtime

The native application's deterministic integration boundary. It is the only
adapter around `dicta_engine::Controller` and translates the shared
`dicta-control` protocol into domain transitions.

Platform work is injected through six small ports. A port may return a result
immediately or mark it pending and deliver it later through the matching
completion method. The runtime itself owns no thread, async executor, UI object,
socket, device, or global state.

Supported control commands:

- `status`, `record status`, `events`
- `record start`, `record stop`, `record toggle`
- annotation `enable`, `disable`, `toggle`, `tool`, `undo`, and `clear`

Other commands receive a stable `invalid_request` response until their domain
services are attached. Consumers can take immutable snapshots and read the
append-only, strictly sequenced protocol event log.

On Unix, `service::LocalRuntimeService` connects the runtime to
`dicta_control::socket::LocalServer`. It serves one client at a time, bounds the
number of requests per connection, preserves request IDs and the runtime-global
event sequence, and relies on the control crate's one-megabyte frame bound.

Startup never removes a live, foreign-owned, permissive, symlinked, or non-socket
path. A private socket is treated as stale only when `connect` returns
`ConnectionRefused` and its device/inode identity is unchanged on a second
metadata check. Shutdown is synchronous and the socket is removed only while its
identity still belongs to this server.

The stoppable runner uses nonblocking accept with a 25 ms default poll interval
and a hard 250 ms configuration ceiling. Its cloneable shutdown handle can stop
an idle service and synchronously clean the socket without async or unsafe code.
Native hosts may also observe newly emitted events without receiving history
replays.

Accepted connections use resumable nonblocking frame polling, so a silent or
slow partial-frame client cannot hold shutdown open. Current limitations: one
client is active at a time, and an in-flight domain command or event observer
must return before shutdown completes. The metadata-check/unlink operation also
has the small pathname race inherent to the standard-library Unix socket API;
platform-specific unsafe syscalls are deliberately excluded.
